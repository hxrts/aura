//! Causality graph computation and analysis

use app_console_types::TraceEvent;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::console_log;

/// Causality graph for trace analysis
#[derive(Debug, Clone)]
pub struct CausalityGraph {
    /// The underlying directed graph
    graph: Graph<u64, CausalityEdge>,
    /// Map from event ID to node index
    event_to_node: HashMap<u64, NodeIndex>,
    /// Map from node index to event ID
    node_to_event: HashMap<NodeIndex, u64>,
}

/// Edge type in the causality graph
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CausalityEdge {
    /// Direct causal dependency (happens-before)
    HappensBefore,
    /// Events from the same participant (program order)
    ProgramOrder,
    /// Concurrent events
    Concurrent,
}

/// Path information in the causality graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityPath {
    /// Event IDs in the path
    pub events: Vec<u64>,
    /// Edge types in the path
    pub edges: Vec<CausalityEdge>,
    /// Total path length
    pub length: usize,
}

/// Causality analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityAnalysis {
    /// Total number of events
    pub total_events: usize,
    /// Number of causal edges
    pub causal_edges: usize,
    /// Number of concurrent edges
    pub concurrent_edges: usize,
    /// Strongly connected components (cycles)
    pub cycles: Vec<Vec<u64>>,
    /// Critical path (longest causal chain)
    pub critical_path: CausalityPath,
}

impl CausalityGraph {
    /// Build causality graph from trace events
    pub fn build(events: &[TraceEvent]) -> Self {
        console_log!("Building causality graph for {} events", events.len());

        let mut graph = Graph::new();
        let mut event_to_node = HashMap::new();
        let mut node_to_event = HashMap::new();

        // Add all events as nodes
        for event in events {
            let node = graph.add_node(event.event_id);
            event_to_node.insert(event.event_id, node);
            node_to_event.insert(node, event.event_id);
        }

        // Add edges based on causality information
        for event in events {
            let current_node = event_to_node[&event.event_id];

            // Add happens-before edges
            for &parent_id in &event.causality.parent_events {
                if let Some(&parent_node) = event_to_node.get(&parent_id) {
                    graph.add_edge(parent_node, current_node, CausalityEdge::HappensBefore);
                }
            }

            for &before_id in &event.causality.happens_before {
                if let Some(&before_node) = event_to_node.get(&before_id) {
                    graph.add_edge(before_node, current_node, CausalityEdge::HappensBefore);
                }
            }

            // Add concurrent edges
            for &concurrent_id in &event.causality.concurrent_with {
                if let Some(&concurrent_node) = event_to_node.get(&concurrent_id) {
                    // Add bidirectional concurrent edges
                    graph.add_edge(current_node, concurrent_node, CausalityEdge::Concurrent);
                    graph.add_edge(concurrent_node, current_node, CausalityEdge::Concurrent);
                }
            }
        }

        // Add program order edges for same participant
        let mut participant_events: HashMap<String, Vec<&TraceEvent>> = HashMap::new();
        for event in events {
            participant_events
                .entry(event.participant.clone())
                .or_default()
                .push(event);
        }

        for (_participant, mut participant_events) in participant_events {
            // Sort by tick to establish program order
            participant_events.sort_by_key(|e| e.tick);

            for window in participant_events.windows(2) {
                let prev_node = event_to_node[&window[0].event_id];
                let next_node = event_to_node[&window[1].event_id];

                // Only add program order if there's no existing causal edge
                if graph.find_edge(prev_node, next_node).is_none() {
                    graph.add_edge(prev_node, next_node, CausalityEdge::ProgramOrder);
                }
            }
        }

        console_log!(
            "Built causality graph: {} nodes, {} edges",
            graph.node_count(),
            graph.edge_count()
        );

        CausalityGraph {
            graph,
            event_to_node,
            node_to_event,
        }
    }

    /// Find the causal path to a specific event
    pub fn path_to(&self, target_event_id: u64) -> Option<CausalityPath> {
        let target_node = self.event_to_node.get(&target_event_id)?;

        // Find the longest path to this event using topological ordering
        let mut distances: HashMap<NodeIndex, (usize, Option<NodeIndex>)> = HashMap::new();
        let mut queue = VecDeque::new();

        // Initialize distances
        for node in self.graph.node_indices() {
            distances.insert(node, (0, None));
            queue.push_back(node);
        }

        // Find longest path using modified Bellman-Ford (since we want longest, not shortest)
        for _ in 0..self.graph.node_count() {
            let mut updated = false;

            for edge in self.graph.edge_indices() {
                if let Some((source, target)) = self.graph.edge_endpoints(edge) {
                    let edge_weight = self.graph[edge];

                    // Only consider causal and program order edges for longest path
                    match edge_weight {
                        CausalityEdge::HappensBefore | CausalityEdge::ProgramOrder => {
                            let source_dist = distances[&source].0;
                            let target_dist = distances[&target].0;

                            if source_dist + 1 > target_dist {
                                distances.insert(target, (source_dist + 1, Some(source)));
                                updated = true;
                            }
                        }
                        CausalityEdge::Concurrent => {
                            // Skip concurrent edges for longest path calculation
                        }
                    }
                }
            }

            if !updated {
                break;
            }
        }

        // Reconstruct path
        let mut path_events = Vec::new();
        let mut path_edges = Vec::new();
        let mut current = *target_node;

        while let Some((_, Some(prev))) = distances.get(&current) {
            path_events.push(self.node_to_event[&current]);

            // Find edge type
            if let Some(edge) = self.graph.find_edge(*prev, current) {
                path_edges.push(self.graph[edge]);
            }

            current = *prev;
        }

        // Add the root event
        path_events.push(self.node_to_event[&current]);

        // Reverse to get path from root to target
        path_events.reverse();
        path_edges.reverse();

        Some(CausalityPath {
            length: path_events.len(),
            events: path_events,
            edges: path_edges,
        })
    }

    /// Get all causal dependencies of an event
    pub fn get_dependencies(&self, event_id: u64) -> Vec<u64> {
        let node = match self.event_to_node.get(&event_id) {
            Some(node) => *node,
            None => return Vec::new(),
        };

        let mut dependencies = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        // BFS backwards through causal edges
        queue.push_back(node);
        visited.insert(node);

        while let Some(current) = queue.pop_front() {
            for edge in self.graph.edges_directed(current, Direction::Incoming) {
                match edge.weight() {
                    CausalityEdge::HappensBefore | CausalityEdge::ProgramOrder => {
                        let source = edge.source();
                        if !visited.contains(&source) {
                            visited.insert(source);
                            queue.push_back(source);
                            dependencies.push(self.node_to_event[&source]);
                        }
                    }
                    CausalityEdge::Concurrent => {
                        // Don't follow concurrent edges for dependencies
                    }
                }
            }
        }

        dependencies.sort();
        dependencies
    }

    /// Get all events that depend on this event
    pub fn get_dependents(&self, event_id: u64) -> Vec<u64> {
        let node = match self.event_to_node.get(&event_id) {
            Some(node) => *node,
            None => return Vec::new(),
        };

        let mut dependents = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        // BFS forwards through causal edges
        queue.push_back(node);
        visited.insert(node);

        while let Some(current) = queue.pop_front() {
            for edge in self.graph.edges_directed(current, Direction::Outgoing) {
                match edge.weight() {
                    CausalityEdge::HappensBefore | CausalityEdge::ProgramOrder => {
                        let target = edge.target();
                        if !visited.contains(&target) {
                            visited.insert(target);
                            queue.push_back(target);
                            dependents.push(self.node_to_event[&target]);
                        }
                    }
                    CausalityEdge::Concurrent => {
                        // Don't follow concurrent edges for dependents
                    }
                }
            }
        }

        dependents.sort();
        dependents
    }

    /// Get events concurrent with the given event
    pub fn get_concurrent_events(&self, event_id: u64) -> Vec<u64> {
        let node = match self.event_to_node.get(&event_id) {
            Some(node) => *node,
            None => return Vec::new(),
        };

        let mut concurrent = Vec::new();

        for edge in self.graph.edges_directed(node, Direction::Outgoing) {
            if matches!(edge.weight(), CausalityEdge::Concurrent) {
                concurrent.push(self.node_to_event[&edge.target()]);
            }
        }

        concurrent.sort();
        concurrent
    }

    /// Analyze the causality graph for patterns and anomalies
    pub fn analyze(&self) -> CausalityAnalysis {
        let total_events = self.graph.node_count();
        let mut causal_edges = 0;
        let mut concurrent_edges = 0;

        // Count edge types
        for edge in self.graph.edge_indices() {
            match self.graph[edge] {
                CausalityEdge::HappensBefore | CausalityEdge::ProgramOrder => causal_edges += 1,
                CausalityEdge::Concurrent => concurrent_edges += 1,
            }
        }

        // Find cycles (shouldn't exist in a proper causality graph)
        let cycles = self.find_cycles();

        // Find critical path (longest causal chain)
        let critical_path = self.find_critical_path();

        CausalityAnalysis {
            total_events,
            causal_edges,
            concurrent_edges,
            cycles,
            critical_path,
        }
    }

    /// Find cycles in the causality graph (indicates errors)
    fn find_cycles(&self) -> Vec<Vec<u64>> {
        // Use DFS to detect cycles
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut cycles = Vec::new();

        for node in self.graph.node_indices() {
            if !visited.contains(&node) {
                self.dfs_cycle_detection(node, &mut visited, &mut rec_stack, &mut cycles);
            }
        }

        cycles
    }

    fn dfs_cycle_detection(
        &self,
        node: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
        rec_stack: &mut HashSet<NodeIndex>,
        cycles: &mut Vec<Vec<u64>>,
    ) {
        visited.insert(node);
        rec_stack.insert(node);

        for edge in self.graph.edges_directed(node, Direction::Outgoing) {
            // Only consider causal edges for cycle detection
            if matches!(
                edge.weight(),
                CausalityEdge::HappensBefore | CausalityEdge::ProgramOrder
            ) {
                let target = edge.target();

                if !visited.contains(&target) {
                    self.dfs_cycle_detection(target, visited, rec_stack, cycles);
                } else if rec_stack.contains(&target) {
                    // Found a cycle - trace back to collect it
                    let cycle = vec![self.node_to_event[&node], self.node_to_event[&target]];
                    cycles.push(cycle);
                }
            }
        }

        rec_stack.remove(&node);
    }

    /// Find the critical path (longest causal chain)
    fn find_critical_path(&self) -> CausalityPath {
        let mut longest_path = CausalityPath {
            events: Vec::new(),
            edges: Vec::new(),
            length: 0,
        };

        // Try finding longest path from each node
        for node in self.graph.node_indices() {
            let event_id = self.node_to_event[&node];
            if let Some(path) = self.path_to(event_id) {
                if path.length > longest_path.length {
                    longest_path = path;
                }
            }
        }

        longest_path
    }

    /// Get summary statistics about the graph
    pub fn get_stats(&self) -> CausalityGraphStats {
        let node_count = self.graph.node_count();
        let edge_count = self.graph.edge_count();

        let mut in_degrees = Vec::new();
        let mut out_degrees = Vec::new();

        for node in self.graph.node_indices() {
            in_degrees.push(self.graph.edges_directed(node, Direction::Incoming).count());
            out_degrees.push(self.graph.edges_directed(node, Direction::Outgoing).count());
        }

        let avg_in_degree = if node_count > 0 {
            in_degrees.iter().sum::<usize>() as f64 / node_count as f64
        } else {
            0.0
        };

        let avg_out_degree = if node_count > 0 {
            out_degrees.iter().sum::<usize>() as f64 / node_count as f64
        } else {
            0.0
        };

        CausalityGraphStats {
            node_count,
            edge_count,
            avg_in_degree,
            avg_out_degree,
            max_in_degree: in_degrees.into_iter().max().unwrap_or(0),
            max_out_degree: out_degrees.into_iter().max().unwrap_or(0),
        }
    }
}

/// Statistics about the causality graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityGraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub avg_in_degree: f64,
    pub avg_out_degree: f64,
    pub max_in_degree: usize,
    pub max_out_degree: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_console_types::{CausalityInfo, EventType};

    fn create_test_event(
        tick: u64,
        event_id: u64,
        participant: &str,
        parent_events: Vec<u64>,
    ) -> TraceEvent {
        TraceEvent {
            tick,
            event_id,
            event_type: EventType::EffectExecuted {
                effect_type: "test".to_string(),
                effect_data: vec![],
            },
            participant: participant.to_string(),
            causality: CausalityInfo {
                parent_events,
                happens_before: vec![],
                concurrent_with: vec![],
            },
        }
    }

    #[test]
    fn test_causality_graph_build() {
        let events = vec![
            create_test_event(0, 1, "alice", vec![]),
            create_test_event(1, 2, "bob", vec![1]),
            create_test_event(2, 3, "alice", vec![2]),
        ];

        let graph = CausalityGraph::build(&events);

        assert_eq!(graph.graph.node_count(), 3);
        assert!(graph.graph.edge_count() >= 2); // At least the causal edges
    }

    #[test]
    fn test_path_finding() {
        let events = vec![
            create_test_event(0, 1, "alice", vec![]),
            create_test_event(1, 2, "bob", vec![1]),
            create_test_event(2, 3, "alice", vec![2]),
        ];

        let graph = CausalityGraph::build(&events);
        let path = graph.path_to(3).unwrap();

        assert_eq!(path.events.len(), 3);
        assert_eq!(path.events[0], 1);
        assert_eq!(path.events[2], 3);
    }

    #[test]
    fn test_dependencies() {
        let events = vec![
            create_test_event(0, 1, "alice", vec![]),
            create_test_event(1, 2, "bob", vec![1]),
            create_test_event(2, 3, "alice", vec![2]),
        ];

        let graph = CausalityGraph::build(&events);
        let deps = graph.get_dependencies(3);

        assert!(deps.contains(&1));
        assert!(deps.contains(&2));
    }
}
