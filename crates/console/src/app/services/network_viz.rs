/// Network Visualization Service
///
/// Handles Cytoscape initialization, graph data management, and JavaScript FFI.
/// Abstracts away the complexity of JavaScript bindings.
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkNode {
    pub id: String,
    pub label: String,
    pub node_type: NodeType,
    pub status: NodeStatus,
    pub position: Option<(f64, f64)>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeType {
    Honest,
    Byzantine,
    Observer,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeStatus {
    Online,
    Offline,
    Syncing,
    Error,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkEdge {
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
    pub strength: f64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EdgeType {
    P2PConnection,
    MessageFlow,
    Trust,
    Partition,
}

#[allow(dead_code)]
#[derive(Default)]
pub struct NetworkVisualization {
    nodes: Vec<NetworkNode>,
    edges: Vec<NetworkEdge>,
}

#[allow(dead_code)]
impl NetworkVisualization {
    pub fn new(nodes: Vec<NetworkNode>, edges: Vec<NetworkEdge>) -> Self {
        Self { nodes, edges }
    }

    /// Get network nodes
    pub fn get_nodes(&self) -> &[NetworkNode] {
        &self.nodes
    }

    /// Get network edges
    pub fn get_edges(&self) -> &[NetworkEdge] {
        &self.edges
    }

    /// Update nodes
    pub fn set_nodes(&mut self, nodes: Vec<NetworkNode>) {
        self.nodes = nodes;
    }

    /// Update edges
    pub fn set_edges(&mut self, edges: Vec<NetworkEdge>) {
        self.edges = edges;
    }

    /// Add a node to the network
    pub fn add_node(&mut self, node: NetworkNode) {
        self.nodes.push(node);
    }

    /// Remove a node by ID
    pub fn remove_node(&mut self, id: &str) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.source != id && e.target != id);
    }

    /// Find node by ID
    pub fn find_node(&self, id: &str) -> Option<&NetworkNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Get all connected nodes for a given node
    pub fn get_neighbors(&self, node_id: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|e| e.source == node_id || e.target == node_id)
            .flat_map(|e| {
                if e.source == node_id {
                    vec![e.target.clone()]
                } else {
                    vec![e.source.clone()]
                }
            })
            .collect()
    }
}

/// Initialize Cytoscape with network data
#[allow(dead_code)]
pub fn init_cytoscape(
    _container: &web_sys::HtmlElement,
    nodes: &[NetworkNode],
    edges: &[NetworkEdge],
) {
    let nodes_json = serde_json::to_string(nodes).unwrap_or_default();
    let edges_json = serde_json::to_string(edges).unwrap_or_default();

    let js_code = format!(
        "const container = arguments[0]; const nodesData = {}; const edgesData = {}; container.innerHTML = ''; const cytoscapeNodes = nodesData.map(n => ({{data: {{id: n.id, label: n.label, type: n.node_type, status: n.status}}, position: n.position ? {{x: n.position[0], y: n.position[1]}} : undefined}})); const cytoscapeEdges = edgesData.map(e => ({{data: {{id: e.source + '-' + e.target, source: e.source, target: e.target, type: e.edge_type, strength: e.strength}}}})); const cy = cytoscape({{container: container, elements: [...cytoscapeNodes, ...cytoscapeEdges], style: [{{selector: 'node', style: {{'width': 60, 'height': 60, 'label': 'data(label)', 'text-valign': 'center', 'text-halign': 'center', 'color': '#333', 'font-size': '12px', 'font-weight': 'bold', 'border-width': 2, 'border-color': '#fff'}}}}], layout: {{name: 'cose', animate: true, animationDuration: 1000, fit: true, padding: 30}}, minZoom: 0.5, maxZoom: 3, wheelSensitivity: 0.2}}); container._cytoscape = cy;",
        nodes_json, edges_json
    );

    let _ = js_sys::eval(&js_code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_visualization_new() {
        let nodes = vec![NetworkNode {
            id: "alice".to_string(),
            label: "Alice".to_string(),
            node_type: NodeType::Honest,
            status: NodeStatus::Online,
            position: None,
        }];
        let edges = vec![];

        let viz = NetworkVisualization::new(nodes, edges);
        assert_eq!(viz.get_nodes().len(), 1);
        assert_eq!(viz.get_edges().len(), 0);
    }

    #[test]
    fn test_find_node() {
        let nodes = vec![NetworkNode {
            id: "alice".to_string(),
            label: "Alice".to_string(),
            node_type: NodeType::Honest,
            status: NodeStatus::Online,
            position: None,
        }];

        let viz = NetworkVisualization::new(nodes, vec![]);
        assert!(viz.find_node("alice").is_some());
        assert!(viz.find_node("bob").is_none());
    }

    #[test]
    fn test_get_neighbors() {
        let nodes = vec![
            NetworkNode {
                id: "alice".to_string(),
                label: "Alice".to_string(),
                node_type: NodeType::Honest,
                status: NodeStatus::Online,
                position: None,
            },
            NetworkNode {
                id: "bob".to_string(),
                label: "Bob".to_string(),
                node_type: NodeType::Honest,
                status: NodeStatus::Online,
                position: None,
            },
        ];
        let edges = vec![NetworkEdge {
            source: "alice".to_string(),
            target: "bob".to_string(),
            edge_type: EdgeType::P2PConnection,
            strength: 1.0,
        }];

        let viz = NetworkVisualization::new(nodes, edges);
        let neighbors = viz.get_neighbors("alice");
        assert_eq!(neighbors.len(), 1);
        assert!(neighbors.contains(&"bob".to_string()));
    }

    #[test]
    fn test_remove_node() {
        let nodes = vec![
            NetworkNode {
                id: "alice".to_string(),
                label: "Alice".to_string(),
                node_type: NodeType::Honest,
                status: NodeStatus::Online,
                position: None,
            },
            NetworkNode {
                id: "bob".to_string(),
                label: "Bob".to_string(),
                node_type: NodeType::Honest,
                status: NodeStatus::Online,
                position: None,
            },
        ];
        let edges = vec![NetworkEdge {
            source: "alice".to_string(),
            target: "bob".to_string(),
            edge_type: EdgeType::P2PConnection,
            strength: 1.0,
        }];

        let mut viz = NetworkVisualization::new(nodes, edges);
        viz.remove_node("alice");

        assert_eq!(viz.get_nodes().len(), 1);
        assert_eq!(viz.get_edges().len(), 0); // Edge should be removed too
    }
}
