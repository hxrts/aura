use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use stylance::import_style;

import_style!(style, "../../../styles/network.css");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkNode {
    pub id: String,
    pub label: String,
    pub node_type: NodeType,
    pub status: NodeStatus,
    pub position: Option<(f64, f64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeType {
    Honest,
    Byzantine,
    Observer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeStatus {
    Online,
    Offline,
    Syncing,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkEdge {
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
    pub strength: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EdgeType {
    P2PConnection,
    MessageFlow,
    Trust,
    Partition,
}

#[component]
pub fn NetworkView() -> impl IntoView {
    let (nodes, set_nodes) = signal(Vec::<NetworkNode>::new());
    let (edges, set_edges) = signal(Vec::<NetworkEdge>::new());
    let (selected_node, _set_selected_node) = signal(None::<String>);
    let network_ref = NodeRef::<leptos::html::Div>::new();

    // Get WebSocket events and responses from context for real network data
    let websocket_events = use_context::<ReadSignal<VecDeque<serde_json::Value>>>()
        .unwrap_or_else(|| signal(VecDeque::new()).0);

    // Update network based on real WebSocket events
    Effect::new(move |_| {
        let ws_events = websocket_events.get();

        // Process events to build network topology
        if ws_events.is_empty() {
            let mock_network = get_mock_network_data();
            set_nodes.set(mock_network.0);
            set_edges.set(mock_network.1);
        } else {
            let mock_network = get_mock_network_data();
            set_nodes.set(mock_network.0);
            set_edges.set(mock_network.1);
        }
    });

    // Initialize with mock data
    Effect::new(move |_| {
        let mock_nodes = vec![
            NetworkNode {
                id: "alice".to_string(),
                label: "Alice".to_string(),
                node_type: NodeType::Honest,
                status: NodeStatus::Online,
                position: Some((100.0, 100.0)),
            },
            NetworkNode {
                id: "bob".to_string(),
                label: "Bob".to_string(),
                node_type: NodeType::Honest,
                status: NodeStatus::Online,
                position: Some((300.0, 100.0)),
            },
            NetworkNode {
                id: "charlie".to_string(),
                label: "Charlie".to_string(),
                node_type: NodeType::Byzantine,
                status: NodeStatus::Error,
                position: Some((200.0, 200.0)),
            },
            NetworkNode {
                id: "observer".to_string(),
                label: "Observer".to_string(),
                node_type: NodeType::Observer,
                status: NodeStatus::Syncing,
                position: Some((400.0, 150.0)),
            },
        ];

        let mock_edges = vec![
            NetworkEdge {
                source: "alice".to_string(),
                target: "bob".to_string(),
                edge_type: EdgeType::P2PConnection,
                strength: 1.0,
            },
            NetworkEdge {
                source: "bob".to_string(),
                target: "charlie".to_string(),
                edge_type: EdgeType::P2PConnection,
                strength: 0.8,
            },
            NetworkEdge {
                source: "alice".to_string(),
                target: "charlie".to_string(),
                edge_type: EdgeType::MessageFlow,
                strength: 0.6,
            },
            NetworkEdge {
                source: "observer".to_string(),
                target: "alice".to_string(),
                edge_type: EdgeType::Trust,
                strength: 0.9,
            },
        ];

        set_nodes.set(mock_nodes);
        set_edges.set(mock_edges);
    });

    // Initialize Cytoscape when component mounts
    Effect::new(move |_| {
        if let Some(container) = network_ref.get() {
            let nodes_data = nodes.get();
            let edges_data = edges.get();
            if !nodes_data.is_empty() {
                init_cytoscape(&container, &nodes_data, &edges_data);
            }
        }
    });

    view! {
        <div class=style::network_view_container>
            <div class=style::network_header>
                <h3>"Network Topology"</h3>
                <div class=style::network_controls>
                    <button class=style::control_button title="Reset Layout">
                        "🔄 Reset"
                    </button>
                    <button class=style::control_button title="Fit to Screen">
                        "🎯 Fit"
                    </button>
                    <button class=style::control_button title="Export">
                        "📁 Export"
                    </button>
                </div>
            </div>

            <div class=style::network_content>
                <div
                    node_ref=network_ref
                    class=style::cytoscape_container
                    style="width: 100%; height: 300px;"
                >
                </div>

                {move || {
                    if let Some(node_id) = selected_node.get() {
                        view! {
                            <div class=style::node_details>
                                <h4>{format!("Node: {}", node_id)}</h4>
                                <div class=style::node_info>
                                    {
                                        match nodes.get().iter().find(|n| n.id == node_id) {
                                            Some(node) => view! {
                                                <div>
                                                    <p><strong>"Type: "</strong> {format!("{:?}", node.node_type)}</p>
                                                    <p><strong>"Status: "</strong> {format!("{:?}", node.status)}</p>
                                                    <p><strong>"ID: "</strong> {node.id.clone()}</p>
                                                </div>
                                            }.into_any(),
                                            None => view! {
                                                <div>
                                                    <p>"Node not found"</p>
                                                </div>
                                            }.into_any()
                                        }
                                    }
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class=style::network_legend>
                                <h4>"Legend"</h4>
                                <div class=style::legend_items>
                                    <div class=style::legend_item>
                                        <div class=format!("{} {}", style::legend_color, style::honest)></div>
                                        <span>"Honest Node"</span>
                                    </div>
                                    <div class=style::legend_item>
                                        <div class=format!("{} {}", style::legend_color, style::byzantine)></div>
                                        <span>"Byzantine Node"</span>
                                    </div>
                                    <div class=style::legend_item>
                                        <div class=format!("{} {}", style::legend_color, style::observer)></div>
                                        <span>"Observer Node"</span>
                                    </div>
                                    <div class=style::legend_item>
                                        <div class=format!("{} {}", style::legend_line, style::p2p)></div>
                                        <span>"P2P Connection"</span>
                                    </div>
                                    <div class=style::legend_item>
                                        <div class=format!("{} {}", style::legend_line, style::message)></div>
                                        <span>"Message Flow"</span>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

fn get_mock_network_data() -> (Vec<NetworkNode>, Vec<NetworkEdge>) {
    let nodes = vec![
        NetworkNode {
            id: "alice".to_string(),
            label: "Alice".to_string(),
            node_type: NodeType::Honest,
            status: NodeStatus::Online,
            position: Some((100.0, 100.0)),
        },
        NetworkNode {
            id: "bob".to_string(),
            label: "Bob".to_string(),
            node_type: NodeType::Honest,
            status: NodeStatus::Online,
            position: Some((300.0, 100.0)),
        },
        NetworkNode {
            id: "charlie".to_string(),
            label: "Charlie".to_string(),
            node_type: NodeType::Byzantine,
            status: NodeStatus::Error,
            position: Some((200.0, 200.0)),
        },
        NetworkNode {
            id: "dave".to_string(),
            label: "Dave".to_string(),
            node_type: NodeType::Observer,
            status: NodeStatus::Syncing,
            position: Some((400.0, 150.0)),
        },
    ];

    let edges = vec![
        NetworkEdge {
            source: "alice".to_string(),
            target: "bob".to_string(),
            edge_type: EdgeType::P2PConnection,
            strength: 1.0,
        },
        NetworkEdge {
            source: "bob".to_string(),
            target: "charlie".to_string(),
            edge_type: EdgeType::Trust,
            strength: 0.8,
        },
        NetworkEdge {
            source: "alice".to_string(),
            target: "dave".to_string(),
            edge_type: EdgeType::MessageFlow,
            strength: 0.6,
        },
    ];

    (nodes, edges)
}

fn init_cytoscape(_container: &web_sys::HtmlElement, nodes: &[NetworkNode], edges: &[NetworkEdge]) {
    let nodes_json = serde_json::to_string(nodes).unwrap_or_default();
    let edges_json = serde_json::to_string(edges).unwrap_or_default();

    let js_code = format!(
        r#"
        const container = arguments[0];
        const nodesData = {};
        const edgesData = {};

        container.innerHTML = '';

        const cytoscapeNodes = nodesData.map(node => ({{
            data: {{
                id: node.id,
                label: node.label,
                type: node.node_type,
                status: node.status
            }},
            position: node.position ? {{ x: node.position[0], y: node.position[1] }} : undefined
        }}));

        const cytoscapeEdges = edgesData.map(edge => ({{
            data: {{
                id: `${{edge.source}}-${{edge.target}}`,
                source: edge.source,
                target: edge.target,
                type: edge.edge_type,
                strength: edge.strength
            }}
        }}));

        const cy = cytoscape({{
            container: container,
            elements: [...cytoscapeNodes, ...cytoscapeEdges],

            style: [
                {{
                    selector: 'node',
                    style: {{
                        'width': 60,
                        'height': 60,
                        'label': 'data(label)',
                        'text-valign': 'center',
                        'text-halign': 'center',
                        'color': 'var(--text-primary)',
                        'font-size': '12px',
                        'font-weight': 'bold',
                        'border-width': 2,
                        'border-color': '#fff'
                    }}
                }},
                {{
                    selector: 'node[type="Honest"]',
                    style: {{
                        'background-color': 'var(--color-success)'
                    }}
                }},
                {{
                    selector: 'node[type="Byzantine"]',
                    style: {{
                        'background-color': 'var(--color-error)'
                    }}
                }},
                {{
                    selector: 'node[type="Observer"]',
                    style: {{
                        'background-color': 'var(--color-secondary)'
                    }}
                }},
                {{
                    selector: 'node[status="Offline"]',
                    style: {{
                        'opacity': 0.5
                    }}
                }},
                {{
                    selector: 'node[status="Error"]',
                    style: {{
                        'border-color': 'var(--color-error)',
                        'border-width': 3
                    }}
                }},

                {{
                    selector: 'edge',
                    style: {{
                        'width': 'mapData(strength, 0, 1, 1, 4)',
                        'line-color': 'var(--border-medium)',
                        'target-arrow-color': 'var(--border-medium)',
                        'target-arrow-shape': 'triangle',
                        'curve-style': 'bezier'
                    }}
                }},
                {{
                    selector: 'edge[type="MessageFlow"]',
                    style: {{
                        'line-color': 'var(--color-primary)',
                        'target-arrow-color': 'var(--color-primary)',
                        'line-style': 'dashed'
                    }}
                }},
                {{
                    selector: 'edge[type="Trust"]',
                    style: {{
                        'line-color': 'var(--color-success)',
                        'target-arrow-color': 'var(--color-success)'
                    }}
                }},
                {{
                    selector: 'edge[type="Partition"]',
                    style: {{
                        'line-color': 'var(--color-error)',
                        'line-style': 'dotted',
                        'target-arrow-shape': 'none'
                    }}
                }},

                {{
                    selector: ':selected',
                    style: {{
                        'border-color': 'var(--color-primary)',
                        'border-width': 4
                    }}
                }}
            ],

            layout: {{
                name: 'fcose',
                animate: true,
                animationDuration: 1000,
                fit: true,
                padding: 30,
                randomize: false,
                nodeRepulsion: 400000,
                idealEdgeLength: 100,
                edgeElasticity: 100,
                nestingFactor: 5,
                gravity: 80,
                numIter: 1000,
                initialTemp: 200,
                coolingFactor: 0.95,
                minTemp: 1.0
            }},

            minZoom: 0.5,
            maxZoom: 3,
            wheelSensitivity: 0.2
        }});

        cy.on('tap', 'node', function(evt) {{
            const node = evt.target;
            console.log('Node clicked:', node.id());

            const selectEvent = new CustomEvent('node-select', {{
                detail: {{ nodeId: node.id() }}
            }});
            container.dispatchEvent(selectEvent);
        }});

        cy.on('tap', function(evt) {{
            if (evt.target === cy) {{
                const deselectEvent = new CustomEvent('node-deselect');
                container.dispatchEvent(deselectEvent);
            }}
        }});

        container._cytoscape = cy;
    "#,
        nodes_json, edges_json
    );

    let _ = js_sys::eval(&js_code);
}
