//! Property causality analysis for tracing violation causes through event chains
//!
//! This module extends the causality graph computation to specifically analyze
//! causality chains leading to property violations. It integrates with the existing
//! causality module to provide detailed analysis of how violations emerge from
//! sequences of events and state transitions.

use crate::causality::{CausalityEdge, CausalityGraph};
use crate::property_monitor::ViolationDetails;
use aura_console_types::{EventType, TraceEvent};
use aura_types::session_utils::properties::PropertyId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use web_sys;

/// Specialized causality analyzer for property violations
#[derive(Debug, Clone)]
pub struct PropertyCausalityAnalyzer {
    /// The underlying causality graph
    causality_graph: CausalityGraph,
    /// Event lookup by ID
    events_by_id: HashMap<u64, TraceEvent>,
    /// Property violations mapped to event IDs
    violation_events: HashMap<PropertyId, Vec<u64>>,
    /// Analysis cache for performance
    analysis_cache: HashMap<CausalityAnalysisKey, PropertyCausalityAnalysis>,
}

/// Key for caching causality analysis results
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CausalityAnalysisKey {
    property_id: PropertyId,
    violation_event_id: u64,
}

/// Comprehensive causality analysis for a property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCausalityAnalysis {
    /// The property that was violated
    pub property_id: PropertyId,
    /// Event ID where the violation occurred
    pub violation_event_id: u64,
    /// Complete causality chain leading to the violation
    pub causality_chain: ViolationCausalityChain,
    /// Contributing factor analysis
    pub contributing_factors: Vec<ContributingFactor>,
    /// Critical events that were necessary for the violation
    pub critical_events: Vec<CriticalEvent>,
    /// Alternative paths that could have prevented the violation
    pub counterfactual_paths: Vec<CounterfactualPath>,
    /// Visualization data for the causality graph
    pub visualization_data: CausalityVisualizationData,
}

/// Detailed causality chain leading to a property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationCausalityChain {
    /// Events in the causality chain, ordered from root causes to violation
    pub events: Vec<CausalityChainEvent>,
    /// Total chain length
    pub chain_length: usize,
    /// Maximum depth from any root cause
    pub max_depth: usize,
    /// Time span from first to last event
    pub time_span_ticks: u64,
    /// Participants involved in the chain
    pub participants: HashSet<String>,
}

/// Event in a causality chain with analysis context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityChainEvent {
    /// Event ID
    pub event_id: u64,
    /// Position in the causality chain (0 = root cause)
    pub chain_position: usize,
    /// Causality depth from any root cause
    pub depth: usize,
    /// Event details
    pub event: TraceEvent,
    /// Edge type to the next event in the chain
    pub edge_to_next: Option<CausalityEdge>,
    /// Contribution score to the violation (0.0 to 1.0)
    pub contribution_score: f64,
    /// Whether this event was a necessary condition for the violation
    pub is_necessary: bool,
}

/// Factor that contributed to a property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributingFactor {
    /// Type of contributing factor
    pub factor_type: ContributingFactorType,
    /// Event IDs involved in this factor
    pub involved_events: Vec<u64>,
    /// Participants involved
    pub participants: Vec<String>,
    /// Impact score (0.0 to 1.0)
    pub impact_score: f64,
    /// Human-readable description
    pub description: String,
    /// Suggested mitigation strategies
    pub mitigation_hints: Vec<String>,
}

/// Types of factors that can contribute to property violations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContributingFactorType {
    /// Network partition isolating participants
    NetworkPartition,
    /// Byzantine behavior from a participant
    ByzantineBehavior,
    /// Message ordering issues
    MessageOrdering,
    /// Race condition between concurrent events
    RaceCondition,
    /// State inconsistency between replicas
    StateInconsistency,
    /// Threshold not met for consensus
    ThresholdFailure,
    /// Cryptographic operation failure
    CryptographicFailure,
    /// Session type violation
    SessionTypeViolation,
    /// Resource exhaustion or limitation
    ResourceExhaustion,
    /// Temporal constraint violation
    TemporalConstraint,
}

/// Critical event that was necessary for the violation to occur
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalEvent {
    /// Event ID
    pub event_id: u64,
    /// Event details
    pub event: TraceEvent,
    /// Why this event was critical
    pub criticality_reason: CriticalityReason,
    /// Events that depend on this critical event
    pub dependent_events: Vec<u64>,
    /// Alternative events that could have replaced this one
    pub alternatives: Vec<AlternativeEvent>,
}

/// Reason why an event was critical for a violation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticalityReason {
    /// Event introduced the inconsistent state
    StateCorruption,
    /// Event enabled a race condition
    RaceConditionTrigger,
    /// Event caused a participant to become Byzantine
    ByzantineTransition,
    /// Event partitioned the network
    NetworkPartition,
    /// Event violated a session type constraint
    SessionTypeViolation,
    /// Event exceeded a resource limit
    ResourceThreshold,
    /// Event violated temporal ordering
    TemporalViolation,
}

/// Alternative event that could have prevented the violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeEvent {
    /// Description of the alternative
    pub description: String,
    /// Event type that could have been sent instead
    pub alternative_event_type: EventType,
    /// Participant who could have sent it
    pub participant: String,
    /// Tick when it should have been sent
    pub tick: u64,
}

/// Path analysis showing how the violation could have been prevented
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterfactualPath {
    /// Description of the prevention strategy
    pub prevention_strategy: String,
    /// Events that would need to be modified
    pub modified_events: Vec<u64>,
    /// Events that would need to be added
    pub added_events: Vec<AlternativeEvent>,
    /// Events that would need to be removed
    pub removed_events: Vec<u64>,
    /// Confidence that this would prevent the violation (0.0 to 1.0)
    pub prevention_confidence: f64,
}

/// Visualization data for property-specific causality graphs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityVisualizationData {
    /// Nodes for visualization
    pub nodes: Vec<CausalityVisualizationNode>,
    /// Edges for visualization
    pub edges: Vec<CausalityVisualizationEdge>,
    /// Layout information
    pub layout: GraphLayout,
    /// Styling information for different event types
    pub styling: GraphStyling,
}

/// Node in the causality visualization graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityVisualizationNode {
    /// Event ID (used as node identifier)
    pub id: u64,
    /// Display label
    pub label: String,
    /// Node type for styling
    pub node_type: VisualizationNodeType,
    /// Position in the graph
    pub position: NodePosition,
    /// Size and styling information
    pub style: NodeStyle,
    /// Additional metadata for tooltips
    pub metadata: HashMap<String, String>,
}

/// Edge in the causality visualization graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityVisualizationEdge {
    /// Source event ID
    pub source: u64,
    /// Target event ID
    pub target: u64,
    /// Edge type
    pub edge_type: CausalityEdge,
    /// Style information
    pub style: EdgeStyle,
    /// Optional label
    pub label: Option<String>,
}

/// Type of node in the visualization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisualizationNodeType {
    /// Root cause event
    RootCause,
    /// Contributing event
    Contributing,
    /// Critical event
    Critical,
    /// Violation event
    Violation,
    /// Normal event
    Normal,
}

/// Position of a node in the visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>, // For 3D visualization
}

/// Styling information for nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStyle {
    /// Node color (hex or named color)
    pub color: String,
    /// Node size
    pub size: f64,
    /// Border color
    pub border_color: String,
    /// Border width
    pub border_width: f64,
    /// Node shape
    pub shape: NodeShape,
}

/// Styling information for edges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeStyle {
    /// Edge color
    pub color: String,
    /// Edge width
    pub width: f64,
    /// Line style
    pub line_style: LineStyle,
    /// Arrow style
    pub arrow_style: ArrowStyle,
}

/// Node shapes for visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeShape {
    Circle,
    Square,
    Diamond,
    Triangle,
    Hexagon,
}

/// Line styles for edges
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineStyle {
    Solid,
    Dashed,
    Dotted,
    DashDot,
}

/// Arrow styles for directed edges
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowStyle {
    Standard,
    Large,
    Small,
    None,
}

/// Graph layout information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphLayout {
    /// Layout algorithm used
    pub algorithm: LayoutAlgorithm,
    /// Bounding box of the graph
    pub bounds: BoundingBox,
    /// Layer information for hierarchical layouts
    pub layers: Vec<GraphLayer>,
}

/// Layout algorithms for graph visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutAlgorithm {
    Hierarchical,
    ForceDirected,
    Circular,
    Grid,
    Timeline,
}

/// Bounding box for the graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// Layer in a hierarchical graph layout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphLayer {
    /// Layer index (0 = root)
    pub layer: usize,
    /// Event IDs in this layer
    pub events: Vec<u64>,
    /// Y coordinate for this layer
    pub y_position: f64,
}

/// Graph styling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStyling {
    /// Color scheme for different node types
    pub color_scheme: HashMap<VisualizationNodeType, String>,
    /// Default node style
    pub default_node_style: NodeStyle,
    /// Default edge style
    pub default_edge_style: EdgeStyle,
    /// Highlight styles for selected elements
    pub highlight_styles: HashMap<String, NodeStyle>,
}

impl PropertyCausalityAnalyzer {
    /// Create a new property causality analyzer from trace events
    pub fn new(events: &[TraceEvent]) -> Self {
        web_sys::console::log_1(
            &format!(
                "Building property causality analyzer for {} events",
                events.len()
            )
            .into(),
        );

        let causality_graph = CausalityGraph::build(events);

        let mut events_by_id = HashMap::new();
        let mut violation_events = HashMap::new();

        for event in events {
            events_by_id.insert(event.event_id, event.clone());

            // Look for property violations in the event type
            if let EventType::PropertyViolation { property, .. } = &event.event_type {
                // Convert property name to PropertyId (this is a simplified approach)
                // In practice, you'd have a proper mapping from property names to IDs
                let property_id = PropertyId::new_v4(); // TODO: Use actual property mapping
                violation_events
                    .entry(property_id)
                    .or_insert_with(Vec::new)
                    .push(event.event_id);
            }
        }

        web_sys::console::log_1(
            &format!(
                "Built analyzer: {} events, {} violation events tracked",
                events_by_id.len(),
                violation_events.values().map(|v| v.len()).sum::<usize>()
            )
            .into(),
        );

        Self {
            causality_graph,
            events_by_id,
            violation_events,
            analysis_cache: HashMap::new(),
        }
    }

    /// Analyze causality leading to a specific property violation
    pub fn analyze_violation_causality(
        &mut self,
        property_id: PropertyId,
        violation_event_id: u64,
    ) -> Option<PropertyCausalityAnalysis> {
        let cache_key = CausalityAnalysisKey {
            property_id,
            violation_event_id,
        };

        // Check cache first
        if let Some(cached_analysis) = self.analysis_cache.get(&cache_key) {
            return Some(cached_analysis.clone());
        }

        web_sys::console::log_1(
            &format!(
                "Analyzing violation causality for property {:?}, event {}",
                property_id, violation_event_id
            )
            .into(),
        );

        // Get the violation event
        let violation_event = self.events_by_id.get(&violation_event_id)?;

        // Build causality chain
        let causality_chain = self.build_causality_chain(violation_event_id)?;

        // Analyze contributing factors
        let contributing_factors = self.analyze_contributing_factors(&causality_chain);

        // Identify critical events
        let critical_events = self.identify_critical_events(&causality_chain);

        // Generate counterfactual paths
        let counterfactual_paths =
            self.generate_counterfactual_paths(&causality_chain, &critical_events);

        // Create visualization data
        let visualization_data = self.create_visualization_data(&causality_chain, &critical_events);

        let analysis = PropertyCausalityAnalysis {
            property_id,
            violation_event_id,
            causality_chain,
            contributing_factors,
            critical_events,
            counterfactual_paths,
            visualization_data,
        };

        // Cache the result
        self.analysis_cache.insert(cache_key, analysis.clone());

        Some(analysis)
    }

    /// Build the complete causality chain leading to a violation
    fn build_causality_chain(&self, violation_event_id: u64) -> Option<ViolationCausalityChain> {
        // Get all dependencies of the violation event
        let dependency_ids = self.causality_graph.get_dependencies(violation_event_id);
        let mut all_event_ids = dependency_ids;
        all_event_ids.push(violation_event_id);

        // Build events with causality information
        let mut events = Vec::new();
        let mut participants = HashSet::new();
        let mut min_tick = u64::MAX;
        let mut max_tick = 0;

        for (position, &event_id) in all_event_ids.iter().enumerate() {
            if let Some(event) = self.events_by_id.get(&event_id) {
                participants.insert(event.participant.clone());
                min_tick = min_tick.min(event.tick);
                max_tick = max_tick.max(event.tick);

                // Calculate contribution score based on position and dependencies
                let contribution_score =
                    self.calculate_contribution_score(event_id, violation_event_id);

                // Determine if this event was necessary
                let is_necessary = self.is_event_necessary(event_id, violation_event_id);

                // Calculate depth from root causes
                let depth = self.calculate_event_depth(event_id);

                // Get edge to next event
                let edge_to_next = if position + 1 < all_event_ids.len() {
                    self.get_edge_between_events(event_id, all_event_ids[position + 1])
                } else {
                    None
                };

                events.push(CausalityChainEvent {
                    event_id,
                    chain_position: position,
                    depth,
                    event: event.clone(),
                    edge_to_next,
                    contribution_score,
                    is_necessary,
                });
            }
        }

        let time_span_ticks = if max_tick >= min_tick {
            max_tick - min_tick
        } else {
            0
        };
        let max_depth = events.iter().map(|e| e.depth).max().unwrap_or(0);

        Some(ViolationCausalityChain {
            events,
            chain_length: all_event_ids.len(),
            max_depth,
            time_span_ticks,
            participants,
        })
    }

    /// Analyze contributing factors to the violation
    fn analyze_contributing_factors(
        &self,
        chain: &ViolationCausalityChain,
    ) -> Vec<ContributingFactor> {
        let mut factors = Vec::new();

        // Analyze network partitions
        factors.extend(self.analyze_network_partitions(chain));

        // Analyze byzantine behavior
        factors.extend(self.analyze_byzantine_behavior(chain));

        // Analyze message ordering issues
        factors.extend(self.analyze_message_ordering(chain));

        // Analyze race conditions
        factors.extend(self.analyze_race_conditions(chain));

        // Analyze state inconsistencies
        factors.extend(self.analyze_state_inconsistencies(chain));

        // Analyze threshold failures
        factors.extend(self.analyze_threshold_failures(chain));

        factors
    }

    /// Identify critical events that were necessary for the violation
    fn identify_critical_events(&self, chain: &ViolationCausalityChain) -> Vec<CriticalEvent> {
        let mut critical_events = Vec::new();

        for chain_event in &chain.events {
            if chain_event.is_necessary {
                let criticality_reason = self.determine_criticality_reason(&chain_event.event);
                let dependent_events = self.causality_graph.get_dependents(chain_event.event_id);
                let alternatives = self.generate_alternative_events(&chain_event.event);

                critical_events.push(CriticalEvent {
                    event_id: chain_event.event_id,
                    event: chain_event.event.clone(),
                    criticality_reason,
                    dependent_events,
                    alternatives,
                });
            }
        }

        critical_events
    }

    /// Generate counterfactual paths showing how violation could be prevented
    fn generate_counterfactual_paths(
        &self,
        chain: &ViolationCausalityChain,
        critical_events: &[CriticalEvent],
    ) -> Vec<CounterfactualPath> {
        let mut paths = Vec::new();

        // For each critical event, generate prevention strategies
        for critical_event in critical_events {
            match critical_event.criticality_reason {
                CriticalityReason::NetworkPartition => {
                    paths.push(self.generate_partition_prevention_path(critical_event));
                }
                CriticalityReason::ByzantineTransition => {
                    paths.push(self.generate_byzantine_prevention_path(critical_event));
                }
                CriticalityReason::RaceConditionTrigger => {
                    paths.push(self.generate_race_prevention_path(critical_event, chain));
                }
                CriticalityReason::StateCorruption => {
                    paths.push(self.generate_state_prevention_path(critical_event));
                }
                _ => {
                    // Generate generic prevention strategy
                    paths.push(self.generate_generic_prevention_path(critical_event));
                }
            }
        }

        paths
    }

    /// Create visualization data for the causality graph
    fn create_visualization_data(
        &self,
        chain: &ViolationCausalityChain,
        critical_events: &[CriticalEvent],
    ) -> CausalityVisualizationData {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let critical_event_ids: HashSet<u64> =
            critical_events.iter().map(|ce| ce.event_id).collect();

        // Create nodes
        for (i, chain_event) in chain.events.iter().enumerate() {
            let node_type = if i == 0 {
                VisualizationNodeType::RootCause
            } else if i == chain.events.len() - 1 {
                VisualizationNodeType::Violation
            } else if critical_event_ids.contains(&chain_event.event_id) {
                VisualizationNodeType::Critical
            } else if chain_event.contribution_score > 0.5 {
                VisualizationNodeType::Contributing
            } else {
                VisualizationNodeType::Normal
            };

            let position = NodePosition {
                x: i as f64 * 100.0, // Simple linear layout
                y: (chain_event.depth as f64) * 50.0,
                z: None,
            };

            let style = self.get_node_style(&node_type);

            let mut metadata = HashMap::new();
            metadata.insert("tick".to_string(), chain_event.event.tick.to_string());
            metadata.insert(
                "participant".to_string(),
                chain_event.event.participant.clone(),
            );
            metadata.insert(
                "contribution".to_string(),
                format!("{:.2}", chain_event.contribution_score),
            );

            nodes.push(CausalityVisualizationNode {
                id: chain_event.event_id,
                label: self.create_event_label(&chain_event.event),
                node_type,
                position,
                style,
                metadata,
            });
        }

        // Create edges
        for chain_event in &chain.events {
            if let Some(edge_type) = chain_event.edge_to_next {
                if let Some(next_event) = chain
                    .events
                    .iter()
                    .find(|e| e.chain_position == chain_event.chain_position + 1)
                {
                    edges.push(CausalityVisualizationEdge {
                        source: chain_event.event_id,
                        target: next_event.event_id,
                        edge_type,
                        style: self.get_edge_style(&edge_type),
                        label: Some(format!("{:?}", edge_type)),
                    });
                }
            }
        }

        let layout = self.create_layout(&nodes);
        let styling = self.create_styling();

        CausalityVisualizationData {
            nodes,
            edges,
            layout,
            styling,
        }
    }

    // Helper methods for analysis

    fn calculate_contribution_score(&self, event_id: u64, violation_event_id: u64) -> f64 {
        // Calculate how much this event contributed to the violation
        // This is a simplified calculation - in practice, this would be more sophisticated
        let dependents = self.causality_graph.get_dependents(event_id);
        if dependents.contains(&violation_event_id) {
            let dependencies = self.causality_graph.get_dependencies(violation_event_id);
            1.0 / (dependencies.len() as f64)
        } else {
            0.0
        }
    }

    fn is_event_necessary(&self, event_id: u64, violation_event_id: u64) -> bool {
        // Determine if removing this event would prevent the violation
        // This is a simplified heuristic - proper analysis would require
        // counterfactual reasoning
        let path = self.causality_graph.path_to(violation_event_id);
        path.map(|p| p.events.contains(&event_id)).unwrap_or(false)
    }

    fn calculate_event_depth(&self, event_id: u64) -> usize {
        // Calculate depth from root causes
        let dependencies = self.causality_graph.get_dependencies(event_id);
        if dependencies.is_empty() {
            0
        } else {
            1 + dependencies
                .iter()
                .map(|&dep_id| self.calculate_event_depth(dep_id))
                .max()
                .unwrap_or(0)
        }
    }

    fn get_edge_between_events(&self, source: u64, target: u64) -> Option<CausalityEdge> {
        // This is a placeholder - the actual implementation would query the causality graph
        Some(CausalityEdge::HappensBefore)
    }

    // Analysis helper methods (simplified implementations)

    fn analyze_network_partitions(
        &self,
        _chain: &ViolationCausalityChain,
    ) -> Vec<ContributingFactor> {
        // Analyze events for network partition patterns
        Vec::new()
    }

    fn analyze_byzantine_behavior(
        &self,
        _chain: &ViolationCausalityChain,
    ) -> Vec<ContributingFactor> {
        // Analyze events for byzantine behavior patterns
        Vec::new()
    }

    fn analyze_message_ordering(
        &self,
        _chain: &ViolationCausalityChain,
    ) -> Vec<ContributingFactor> {
        // Analyze message ordering issues
        Vec::new()
    }

    fn analyze_race_conditions(&self, _chain: &ViolationCausalityChain) -> Vec<ContributingFactor> {
        // Analyze concurrent events for race conditions
        Vec::new()
    }

    fn analyze_state_inconsistencies(
        &self,
        _chain: &ViolationCausalityChain,
    ) -> Vec<ContributingFactor> {
        // Analyze CRDT merges and state transitions
        Vec::new()
    }

    fn analyze_threshold_failures(
        &self,
        _chain: &ViolationCausalityChain,
    ) -> Vec<ContributingFactor> {
        // Analyze threshold signature failures
        Vec::new()
    }

    fn determine_criticality_reason(&self, event: &TraceEvent) -> CriticalityReason {
        // Determine why this event was critical based on its type
        match &event.event_type {
            EventType::MessageDropped { reason, .. } => match reason {
                aura_console_types::trace::DropReason::NetworkPartition => {
                    CriticalityReason::NetworkPartition
                }
                _ => CriticalityReason::StateCorruption,
            },
            EventType::CrdtMerge { .. } => CriticalityReason::StateCorruption,
            EventType::ProtocolStateTransition { .. } => CriticalityReason::SessionTypeViolation,
            _ => CriticalityReason::StateCorruption,
        }
    }

    fn generate_alternative_events(&self, _event: &TraceEvent) -> Vec<AlternativeEvent> {
        // Generate alternative events that could have been sent instead
        Vec::new()
    }

    // Counterfactual path generation methods

    fn generate_partition_prevention_path(
        &self,
        _critical_event: &CriticalEvent,
    ) -> CounterfactualPath {
        CounterfactualPath {
            prevention_strategy: "Maintain network connectivity".to_string(),
            modified_events: Vec::new(),
            added_events: Vec::new(),
            removed_events: Vec::new(),
            prevention_confidence: 0.8,
        }
    }

    fn generate_byzantine_prevention_path(
        &self,
        _critical_event: &CriticalEvent,
    ) -> CounterfactualPath {
        CounterfactualPath {
            prevention_strategy: "Prevent byzantine behavior".to_string(),
            modified_events: Vec::new(),
            added_events: Vec::new(),
            removed_events: Vec::new(),
            prevention_confidence: 0.7,
        }
    }

    fn generate_race_prevention_path(
        &self,
        _critical_event: &CriticalEvent,
        _chain: &ViolationCausalityChain,
    ) -> CounterfactualPath {
        CounterfactualPath {
            prevention_strategy: "Synchronize concurrent operations".to_string(),
            modified_events: Vec::new(),
            added_events: Vec::new(),
            removed_events: Vec::new(),
            prevention_confidence: 0.6,
        }
    }

    fn generate_state_prevention_path(
        &self,
        _critical_event: &CriticalEvent,
    ) -> CounterfactualPath {
        CounterfactualPath {
            prevention_strategy: "Maintain state consistency".to_string(),
            modified_events: Vec::new(),
            added_events: Vec::new(),
            removed_events: Vec::new(),
            prevention_confidence: 0.9,
        }
    }

    fn generate_generic_prevention_path(
        &self,
        _critical_event: &CriticalEvent,
    ) -> CounterfactualPath {
        CounterfactualPath {
            prevention_strategy: "General violation prevention".to_string(),
            modified_events: Vec::new(),
            added_events: Vec::new(),
            removed_events: Vec::new(),
            prevention_confidence: 0.5,
        }
    }

    // Visualization helper methods

    fn get_node_style(&self, node_type: &VisualizationNodeType) -> NodeStyle {
        let (color, size) = match node_type {
            VisualizationNodeType::RootCause => ("#ff4444".to_string(), 20.0),
            VisualizationNodeType::Critical => ("#ff8800".to_string(), 15.0),
            VisualizationNodeType::Contributing => ("#ffcc00".to_string(), 12.0),
            VisualizationNodeType::Violation => ("#cc0000".to_string(), 25.0),
            VisualizationNodeType::Normal => ("#888888".to_string(), 10.0),
        };

        NodeStyle {
            color,
            size,
            border_color: "#000000".to_string(),
            border_width: 1.0,
            shape: NodeShape::Circle,
        }
    }

    fn get_edge_style(&self, edge_type: &CausalityEdge) -> EdgeStyle {
        let (color, width, line_style) = match edge_type {
            CausalityEdge::HappensBefore => ("#000000".to_string(), 2.0, LineStyle::Solid),
            CausalityEdge::ProgramOrder => ("#666666".to_string(), 1.0, LineStyle::Dashed),
            CausalityEdge::Concurrent => ("#cccccc".to_string(), 1.0, LineStyle::Dotted),
        };

        EdgeStyle {
            color,
            width,
            line_style,
            arrow_style: ArrowStyle::Standard,
        }
    }

    fn create_event_label(&self, event: &TraceEvent) -> String {
        match &event.event_type {
            EventType::ProtocolStateTransition {
                protocol, to_state, ..
            } => {
                format!("{}→{}", protocol, to_state)
            }
            EventType::MessageSent { message_type, .. } => {
                format!("Send {}", message_type)
            }
            EventType::MessageReceived { message_type, .. } => {
                format!("Recv {}", message_type)
            }
            EventType::PropertyViolation { property, .. } => {
                format!("Violation: {}", property)
            }
            _ => format!("Event {}", event.event_id),
        }
    }

    fn create_layout(&self, nodes: &[CausalityVisualizationNode]) -> GraphLayout {
        let mut layers = HashMap::new();
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for node in nodes {
            min_x = min_x.min(node.position.x);
            max_x = max_x.max(node.position.x);
            min_y = min_y.min(node.position.y);
            max_y = max_y.max(node.position.y);

            let layer = (node.position.y / 50.0) as usize;
            layers.entry(layer).or_insert_with(Vec::new).push(node.id);
        }

        let graph_layers = layers
            .into_iter()
            .map(|(layer, events)| GraphLayer {
                layer,
                events,
                y_position: layer as f64 * 50.0,
            })
            .collect();

        GraphLayout {
            algorithm: LayoutAlgorithm::Hierarchical,
            bounds: BoundingBox {
                min_x,
                min_y,
                max_x,
                max_y,
            },
            layers: graph_layers,
        }
    }

    fn create_styling(&self) -> GraphStyling {
        let mut color_scheme = HashMap::new();
        color_scheme.insert(VisualizationNodeType::RootCause, "#ff4444".to_string());
        color_scheme.insert(VisualizationNodeType::Critical, "#ff8800".to_string());
        color_scheme.insert(VisualizationNodeType::Contributing, "#ffcc00".to_string());
        color_scheme.insert(VisualizationNodeType::Violation, "#cc0000".to_string());
        color_scheme.insert(VisualizationNodeType::Normal, "#888888".to_string());

        GraphStyling {
            color_scheme,
            default_node_style: NodeStyle {
                color: "#888888".to_string(),
                size: 10.0,
                border_color: "#000000".to_string(),
                border_width: 1.0,
                shape: NodeShape::Circle,
            },
            default_edge_style: EdgeStyle {
                color: "#666666".to_string(),
                width: 1.0,
                line_style: LineStyle::Solid,
                arrow_style: ArrowStyle::Standard,
            },
            highlight_styles: HashMap::new(),
        }
    }

    /// Get all property violations that have been analyzed
    pub fn get_analyzed_violations(&self) -> Vec<(PropertyId, Vec<u64>)> {
        self.violation_events
            .iter()
            .map(|(id, events)| (*id, events.clone()))
            .collect()
    }

    /// Get causality paths for all violations of a specific property
    pub fn get_property_violation_paths(
        &mut self,
        property_id: PropertyId,
    ) -> Vec<PropertyCausalityAnalysis> {
        if let Some(violation_event_ids) = self.violation_events.get(&property_id).cloned() {
            violation_event_ids
                .iter()
                .filter_map(|&event_id| self.analyze_violation_causality(property_id, event_id))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Clear the analysis cache
    pub fn clear_cache(&mut self) {
        self.analysis_cache.clear();
        web_sys::console::log_1(&"Property causality analysis cache cleared".into());
    }

    /// Get statistics about the analyzer
    pub fn get_stats(&self) -> PropertyCausalityStats {
        PropertyCausalityStats {
            total_events: self.events_by_id.len(),
            total_violations: self.violation_events.values().map(|v| v.len()).sum(),
            properties_with_violations: self.violation_events.len(),
            cached_analyses: self.analysis_cache.len(),
        }
    }
}

/// Statistics about the property causality analyzer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCausalityStats {
    /// Total number of events in the trace
    pub total_events: usize,
    /// Total number of property violations
    pub total_violations: usize,
    /// Number of properties that have violations
    pub properties_with_violations: usize,
    /// Number of cached causality analyses
    pub cached_analyses: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_console_types::{CausalityInfo, EventType};

    fn create_test_event(
        tick: u64,
        event_id: u64,
        participant: &str,
        event_type: EventType,
        parent_events: Vec<u64>,
    ) -> TraceEvent {
        TraceEvent {
            tick,
            event_id,
            event_type,
            participant: participant.to_string(),
            causality: CausalityInfo {
                parent_events,
                happens_before: vec![],
                concurrent_with: vec![],
            },
        }
    }

    #[test]
    fn test_property_causality_analyzer_creation() {
        let events = vec![
            create_test_event(
                0,
                1,
                "alice",
                EventType::EffectExecuted {
                    effect_type: "test".to_string(),
                    effect_data: vec![],
                },
                vec![],
            ),
            create_test_event(
                1,
                2,
                "bob",
                EventType::PropertyViolation {
                    property: "safety".to_string(),
                    violation_details: "Test violation".to_string(),
                },
                vec![1],
            ),
        ];

        let analyzer = PropertyCausalityAnalyzer::new(&events);

        assert_eq!(analyzer.events_by_id.len(), 2);
        let stats = analyzer.get_stats();
        assert_eq!(stats.total_events, 2);
    }

    #[test]
    fn test_causality_chain_building() {
        let events = vec![
            create_test_event(
                0,
                1,
                "alice",
                EventType::EffectExecuted {
                    effect_type: "init".to_string(),
                    effect_data: vec![],
                },
                vec![],
            ),
            create_test_event(
                1,
                2,
                "bob",
                EventType::MessageSent {
                    envelope_id: "msg1".to_string(),
                    to: vec!["charlie".to_string()],
                    message_type: "proposal".to_string(),
                    size_bytes: 100,
                },
                vec![1],
            ),
            create_test_event(
                2,
                3,
                "charlie",
                EventType::PropertyViolation {
                    property: "consensus".to_string(),
                    violation_details: "Consensus failed".to_string(),
                },
                vec![2],
            ),
        ];

        let mut analyzer = PropertyCausalityAnalyzer::new(&events);
        let property_id = PropertyId::new_v4();

        // This would typically find the violation through the violation_events mapping
        // For testing, we'll check the basic structure
        assert_eq!(analyzer.events_by_id.len(), 3);
    }
}
