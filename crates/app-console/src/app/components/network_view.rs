use crate::app::services::data_source::{use_data_source, DataSource};
use crate::app::services::mock_data::{NetworkEdge, NetworkNode};
use leptos::prelude::*;
use std::collections::VecDeque;

#[component]
pub fn NetworkView() -> impl IntoView {
    log::info!("NetworkView component mounting...");

    let data_source_manager = use_data_source();
    let current_source = data_source_manager.current_source();

    log::info!("Initial data source: {:?}", current_source.get_untracked());

    let (nodes, set_nodes) = signal(Vec::<NetworkNode>::new());
    let (_edges, set_edges) = signal(Vec::<NetworkEdge>::new());
    let (selected_node, _set_selected_node) = signal(None::<String>);
    let network_ref = NodeRef::<leptos::html::Div>::new();

    // Track if we've already initialized to prevent re-rendering loops
    let initialized = RwSignal::new(false);

    // Immediate test: load mock data right now
    log::info!("Loading mock data immediately for testing...");
    use crate::app::services::mock_data::get_mock_network_data;
    let (initial_nodes, initial_edges) = get_mock_network_data();
    log::info!(
        "Got {} nodes and {} edges from direct call",
        initial_nodes.len(),
        initial_edges.len()
    );
    set_nodes.set(initial_nodes.clone());
    set_edges.set(initial_edges.clone());

    // Get WebSocket events and responses from context for real network data
    let websocket_events = use_context::<ReadSignal<VecDeque<serde_json::Value>>>()
        .unwrap_or_else(|| signal(VecDeque::new()).0);

    // Update network data when data source changes
    Effect::new(move |_| {
        let source = current_source.get();
        log::info!("NetworkView updating for data source: {:?}", source);

        // Test mock data directly
        if source == DataSource::Mock {
            log::info!("Testing mock data directly...");
            use crate::app::services::mock_data::get_mock_network_data;
            let (test_nodes, test_edges) = get_mock_network_data();
            log::info!(
                "Direct mock data call: {} nodes, {} edges",
                test_nodes.len(),
                test_edges.len()
            );
        }

        match source {
            DataSource::Mock | DataSource::Simulator | DataSource::Real => {
                let service = data_source_manager.get_service();
                let (network_nodes, network_edges) = service.get_network_data();
                log::info!(
                    "NetworkView received {} nodes and {} edges",
                    network_nodes.len(),
                    network_edges.len()
                );

                // Debug: Log the actual node data
                for node in &network_nodes {
                    log::info!("Node: {} ({})", node.label, node.id);
                }
                for edge in &network_edges {
                    log::info!("Edge: {} -> {}", edge.source, edge.target);
                }

                set_nodes.set(network_nodes.clone());
                set_edges.set(network_edges.clone());

                // Trigger initialization immediately after data is loaded
                if !initialized.get_untracked() {
                    if let Some(container) = network_ref.get_untracked() {
                        log::info!("Data loaded, initializing Cytoscape immediately");
                        init_cytoscape(&container, &network_nodes, &network_edges);
                        initialized.set(true);
                    }
                }
            }
        }
    });

    // WebSocket events effect (for real-time data when available)
    Effect::new(move |_| {
        if current_source.get() == DataSource::Real {
            let _ws_events = websocket_events.get();
            // TODO: Process WebSocket events for real network topology updates
            // For now, we'll let the data source service handle this
        }
    });

    view! {
        <div class="flex flex-col h-full gap-3">
            <div class="flex items-center justify-between gap-2 flex-shrink-0">
                <h3 class="heading-1">"Network Topology"</h3>
                <div class="flex gap-2">
                    <button class="btn-secondary btn-sm" title="Reset Layout">
                        "Reset"
                    </button>
                    <button class="btn-secondary btn-sm" title="Fit to Screen">
                        "Fit"
                    </button>
                </div>
            </div>

            <div class="flex-1 flex flex-col gap-3 min-h-0">
                <div
                    node_ref=network_ref
                    class="card-secondary"
                    style="min-height: 500px; height: 500px;"
                >
                </div>

                {move || {
                    if let Some(node_id) = selected_node.get() {
                        view! {
                            <div class="node-details">
                                <h4>{format!("Node: {}", node_id)}</h4>
                                <div class="node-info">
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
                            <div class="card-secondary card-compact">
                                <h4 class="heading-4 mb-2">"Legend"</h4>
                                <div class="grid-cols-auto">
                                    <div class="flex items-center gap-2">
                                        <div class="w-3 h-3 rounded-full bg-green-500 flex-shrink-0"></div>
                                        <span class="text-tertiary text-xs">"Honest"</span>
                                    </div>
                                    <div class="flex items-center gap-2">
                                        <div class="w-3 h-3 rounded-full bg-red-500 flex-shrink-0"></div>
                                        <span class="text-tertiary text-xs">"Byzantine"</span>
                                    </div>
                                    <div class="flex items-center gap-2">
                                        <div class="w-3 h-3 rounded-full bg-blue-500 flex-shrink-0"></div>
                                        <span class="text-tertiary text-xs">"Observer"</span>
                                    </div>
                                    <div class="flex items-center gap-2">
                                        <div class="flex items-center flex-shrink-0">
                                            <div class="w-5 h-px bg-zinc-800 dark:bg-zinc-400"></div>
                                            <div class="w-0 h-0 border-t-[3px] border-t-transparent border-b-[3px] border-b-transparent border-l-[4px] border-l-zinc-800 dark:border-l-zinc-400"></div>
                                        </div>
                                        <span class="text-tertiary text-xs">"P2P"</span>
                                    </div>
                                    <div class="flex items-center gap-2">
                                        <div class="w-6 h-px bg-blue-400 flex-shrink-0" style="border-top: 2px dashed currentColor;"></div>
                                        <span class="text-tertiary text-xs">"Message"</span>
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

fn init_cytoscape(container: &web_sys::HtmlElement, nodes: &[NetworkNode], edges: &[NetworkEdge]) {
    log::info!(
        "Initializing Cytoscape with {} nodes and {} edges",
        nodes.len(),
        edges.len()
    );

    // Check container dimensions
    let client_width = container.client_width();
    let client_height = container.client_height();
    log::info!("Container dimensions: {}x{}", client_width, client_height);

    // Clear container first
    container.set_inner_html("");

    // Add a visible marker to confirm the container exists

    // Use a simpler approach to initialize Cytoscape without eval
    use wasm_bindgen::prelude::*;
    use web_sys::*;

    // Try to get cytoscape from window
    let window = web_sys::window().unwrap();

    // Check if cytoscape is available
    let cytoscape_fn = js_sys::Reflect::get(&window, &JsValue::from_str("cytoscape"));

    match cytoscape_fn {
        Ok(cytoscape_ref) if !cytoscape_ref.is_undefined() => {
            // Cytoscape is available, create visualization
            log::info!("Cytoscape library found, initializing visualization");

            // Create elements array for cytoscape
            let elements = js_sys::Array::new();

            // Add nodes
            for node in nodes {
                let node_obj = js_sys::Object::new();
                let data_obj = js_sys::Object::new();

                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("id"),
                    &JsValue::from_str(&node.id),
                )
                .unwrap();
                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("label"),
                    &JsValue::from_str(&node.label),
                )
                .unwrap();
                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str(&format!("{:?}", node.node_type)),
                )
                .unwrap();
                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("status"),
                    &JsValue::from_str(&format!("{:?}", node.status)),
                )
                .unwrap();

                js_sys::Reflect::set(&node_obj, &JsValue::from_str("data"), &data_obj).unwrap();
                elements.push(&node_obj);
            }

            // Add edges
            for edge in edges {
                let edge_obj = js_sys::Object::new();
                let data_obj = js_sys::Object::new();

                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("id"),
                    &JsValue::from_str(&format!("{}-{}", edge.source, edge.target)),
                )
                .unwrap();
                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("source"),
                    &JsValue::from_str(&edge.source),
                )
                .unwrap();
                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("target"),
                    &JsValue::from_str(&edge.target),
                )
                .unwrap();
                js_sys::Reflect::set(
                    &data_obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str(&format!("{:?}", edge.edge_type)),
                )
                .unwrap();

                js_sys::Reflect::set(&edge_obj, &JsValue::from_str("data"), &data_obj).unwrap();
                elements.push(&edge_obj);
            }

            // Create cytoscape configuration
            let config = js_sys::Object::new();
            js_sys::Reflect::set(
                &config,
                &JsValue::from_str("container"),
                &JsValue::from(container),
            )
            .unwrap();
            js_sys::Reflect::set(&config, &JsValue::from_str("elements"), &elements).unwrap();

            // Add style configuration
            let style = create_cytoscape_style();
            js_sys::Reflect::set(&config, &JsValue::from_str("style"), &style).unwrap();

            // Add layout configuration
            let layout = create_cytoscape_layout();
            js_sys::Reflect::set(&config, &JsValue::from_str("layout"), &layout).unwrap();

            // Try to call cytoscape
            let cytoscape_result =
                js_sys::Function::from(cytoscape_ref).call1(&JsValue::NULL, &config);

            match cytoscape_result {
                Ok(_cy_instance) => {
                    log::info!("Cytoscape initialized successfully with direct API");
                }
                Err(e) => {
                    log::error!("Failed to initialize cytoscape: {:?}", e);
                    show_fallback_display(container, nodes, edges);
                }
            }
        }
        _ => {
            log::warn!("Cytoscape library not found, showing fallback display");
            show_fallback_display(container, nodes, edges);
        }
    }
}

fn create_cytoscape_style() -> js_sys::Array {
    use wasm_bindgen::prelude::*;

    let style = js_sys::Array::new();

    // Node style
    let node_style = js_sys::Object::new();
    js_sys::Reflect::set(
        &node_style,
        &JsValue::from_str("selector"),
        &JsValue::from_str("node"),
    )
    .unwrap();
    let node_style_props = js_sys::Object::new();
    js_sys::Reflect::set(
        &node_style_props,
        &JsValue::from_str("width"),
        &JsValue::from_f64(60.0),
    )
    .unwrap();
    js_sys::Reflect::set(
        &node_style_props,
        &JsValue::from_str("height"),
        &JsValue::from_f64(60.0),
    )
    .unwrap();
    js_sys::Reflect::set(
        &node_style_props,
        &JsValue::from_str("label"),
        &JsValue::from_str("data(label)"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &node_style_props,
        &JsValue::from_str("text-valign"),
        &JsValue::from_str("center"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &node_style_props,
        &JsValue::from_str("text-halign"),
        &JsValue::from_str("center"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &node_style_props,
        &JsValue::from_str("font-size"),
        &JsValue::from_str("12px"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &node_style_props,
        &JsValue::from_str("background-color"),
        &JsValue::from_str("#94a3b8"),
    )
    .unwrap();
    js_sys::Reflect::set(&node_style, &JsValue::from_str("style"), &node_style_props).unwrap();
    style.push(&node_style);

    // Honest node style
    let honest_style = js_sys::Object::new();
    js_sys::Reflect::set(
        &honest_style,
        &JsValue::from_str("selector"),
        &JsValue::from_str("node[type='Honest']"),
    )
    .unwrap();
    let honest_props = js_sys::Object::new();
    js_sys::Reflect::set(
        &honest_props,
        &JsValue::from_str("background-color"),
        &JsValue::from_str("#22c55e"),
    )
    .unwrap();
    js_sys::Reflect::set(&honest_style, &JsValue::from_str("style"), &honest_props).unwrap();
    style.push(&honest_style);

    // Byzantine node style
    let byzantine_style = js_sys::Object::new();
    js_sys::Reflect::set(
        &byzantine_style,
        &JsValue::from_str("selector"),
        &JsValue::from_str("node[type='Byzantine']"),
    )
    .unwrap();
    let byzantine_props = js_sys::Object::new();
    js_sys::Reflect::set(
        &byzantine_props,
        &JsValue::from_str("background-color"),
        &JsValue::from_str("#ef4444"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &byzantine_style,
        &JsValue::from_str("style"),
        &byzantine_props,
    )
    .unwrap();
    style.push(&byzantine_style);

    // Observer node style
    let observer_style = js_sys::Object::new();
    js_sys::Reflect::set(
        &observer_style,
        &JsValue::from_str("selector"),
        &JsValue::from_str("node[type='Observer']"),
    )
    .unwrap();
    let observer_props = js_sys::Object::new();
    js_sys::Reflect::set(
        &observer_props,
        &JsValue::from_str("background-color"),
        &JsValue::from_str("#3b82f6"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &observer_style,
        &JsValue::from_str("style"),
        &observer_props,
    )
    .unwrap();
    style.push(&observer_style);

    // Edge style
    let edge_style = js_sys::Object::new();
    js_sys::Reflect::set(
        &edge_style,
        &JsValue::from_str("selector"),
        &JsValue::from_str("edge"),
    )
    .unwrap();
    let edge_props = js_sys::Object::new();
    js_sys::Reflect::set(
        &edge_props,
        &JsValue::from_str("width"),
        &JsValue::from_f64(2.0),
    )
    .unwrap();
    js_sys::Reflect::set(
        &edge_props,
        &JsValue::from_str("line-color"),
        &JsValue::from_str("#94a3b8"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &edge_props,
        &JsValue::from_str("target-arrow-color"),
        &JsValue::from_str("#94a3b8"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &edge_props,
        &JsValue::from_str("target-arrow-shape"),
        &JsValue::from_str("triangle"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &edge_props,
        &JsValue::from_str("curve-style"),
        &JsValue::from_str("bezier"),
    )
    .unwrap();
    js_sys::Reflect::set(&edge_style, &JsValue::from_str("style"), &edge_props).unwrap();
    style.push(&edge_style);

    style
}

fn create_cytoscape_layout() -> js_sys::Object {
    use wasm_bindgen::prelude::*;

    let layout = js_sys::Object::new();
    js_sys::Reflect::set(
        &layout,
        &JsValue::from_str("name"),
        &JsValue::from_str("cose"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &layout,
        &JsValue::from_str("animate"),
        &JsValue::from_bool(true),
    )
    .unwrap();
    js_sys::Reflect::set(
        &layout,
        &JsValue::from_str("fit"),
        &JsValue::from_bool(true),
    )
    .unwrap();
    js_sys::Reflect::set(
        &layout,
        &JsValue::from_str("padding"),
        &JsValue::from_f64(30.0),
    )
    .unwrap();
    js_sys::Reflect::set(
        &layout,
        &JsValue::from_str("nodeRepulsion"),
        &JsValue::from_f64(400000.0),
    )
    .unwrap();
    js_sys::Reflect::set(
        &layout,
        &JsValue::from_str("idealEdgeLength"),
        &JsValue::from_f64(100.0),
    )
    .unwrap();

    layout
}

fn show_fallback_display(
    container: &web_sys::HtmlElement,
    nodes: &[NetworkNode],
    edges: &[NetworkEdge],
) {
    let node_list = nodes
        .iter()
        .map(|n| format!("• {} ({:?}, {:?})", n.label, n.node_type, n.status))
        .collect::<Vec<_>>()
        .join("<br>");

    let edge_list = edges
        .iter()
        .map(|e| format!("• {} → {} ({:?})", e.source, e.target, e.edge_type))
        .collect::<Vec<_>>()
        .join("<br>");

    container.set_inner_html(&format!(
        r#"<div style="padding: 20px; color: #333; background: #f9f9f9; border-radius: 8px; height: 100%; overflow: auto;">
            <h4 style="margin: 0 0 16px 0; color: #555;">Network Topology (Data View)</h4>
            <div style="margin-bottom: 16px;">
                <strong>Nodes ({}):</strong><br>
                <div style="margin: 8px 0; font-size: 14px; line-height: 1.4;">{}</div>
            </div>
            <div style="margin-bottom: 16px;">
                <strong>Edges ({}):</strong><br>
                <div style="margin: 8px 0; font-size: 14px; line-height: 1.4;">{}</div>
            </div>
            <div style="margin-top: 16px; padding: 8px; background: #d1ecf1; border-radius: 4px; font-size: 12px; color: #0c5460;">
                Mock data is loading correctly! Network visualization shows: {} nodes, {} edges
            </div>
        </div>"#,
        nodes.len(),
        node_list,
        edges.len(),
        edge_list,
        nodes.len(),
        edges.len()
    ));
}
