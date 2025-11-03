use crate::app::components::{ChevronDown, ChevronRight};
use crate::app::services::data_source::{use_data_source, DataSource};
use crate::app::services::mock_data::StateData;
use crate::app::services::ConnectionState;
use leptos::prelude::*;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct JsonTreeNode {
    pub key: String,
    pub value: Value,
    pub expanded: bool,
    pub path: String,
}

#[component]
pub fn StateInspector() -> impl IntoView {
    let data_source_manager = use_data_source();
    let current_source = data_source_manager.current_source();
    let data_source_manager_for_effect = data_source_manager.clone();
    let data_source_manager_for_view = data_source_manager.clone();

    let current_state = RwSignal::new(None::<StateData>);
    let expanded_paths = RwSignal::new(HashMap::<String, bool>::new());
    let search_term = RwSignal::new(String::new());
    let view_mode = RwSignal::new(ViewMode::Tree);

    let websocket_responses = use_context::<ReadSignal<VecDeque<serde_json::Value>>>()
        .unwrap_or_else(|| signal(VecDeque::new()).0);

    // Update state when data source changes
    Effect::new(move |_| {
        let source = current_source.get();
        log::info!("StateInspector updating for data source: {:?}", source);

        let service = data_source_manager_for_effect.get_service();
        let connection_status = service.get_connection_status();

        match source {
            DataSource::Mock => {
                // Mock always shows data
                let state_data = service.get_state_data();
                log::info!(
                    "StateInspector: Setting mock state data for node: {}",
                    state_data.node_id
                );
                current_state.set(Some(state_data));
            }
            DataSource::Simulator | DataSource::Real => {
                // Only show data if connected
                if matches!(connection_status, ConnectionState::Connected) {
                    let state_data = service.get_state_data();
                    log::info!(
                        "StateInspector: Setting {} state data for node: {}",
                        source,
                        state_data.node_id
                    );
                    current_state.set(Some(state_data));
                } else {
                    log::info!("StateInspector: Clearing state data - {} source not connected (status: {:?})", source, connection_status);
                    current_state.set(None);
                }
            }
        }
    });

    // WebSocket responses effect (for real-time data when available)
    Effect::new(move |_| {
        if current_source.get() == DataSource::Real {
            let responses = websocket_responses.get();
            for response in responses.iter() {
                if let Ok(state_data) = serde_json::from_value::<StateData>(response.clone()) {
                    current_state.set(Some(state_data));
                    return;
                }
            }
        }
    });

    view! {
        <div class="flex flex-col h-full gap-3">
            <div class="section-header">
                <h3 class="heading-1">"State Inspector"</h3>
                <div class="flex gap-3">
                    <div class="inline-flex gap-1 bg-secondary rounded-md p-1">
                        <button
                            class=move || if view_mode.get() == ViewMode::Tree {
                                "btn-sm rounded bg-white dark:bg-zinc-700 text-primary border border-primary shadow-sm"
                            } else {
                                "btn-sm rounded text-tertiary border border-transparent hover:bg-white dark:hover:bg-zinc-700 transition-colors"
                            }
                            on:click=move |_| view_mode.set(ViewMode::Tree)
                        >
                            "Tree"
                        </button>
                        <button
                            class=move || if view_mode.get() == ViewMode::Raw {
                                "btn-sm rounded bg-white dark:bg-zinc-700 text-primary border border-primary shadow-sm"
                            } else {
                                "btn-sm rounded text-tertiary border border-transparent hover:bg-white dark:hover:bg-zinc-700 transition-colors"
                            }
                            on:click=move |_| view_mode.set(ViewMode::Raw)
                        >
                            "Raw"
                        </button>
                    </div>
                    <input
                        type="text"
                        placeholder="Search state..."
                        class="input-base w-64"
                        prop:value=move || search_term.get()
                        on:input=move |ev| {
                            search_term.set(event_target_value(&ev));
                        }
                    />
                </div>
            </div>

            <div class="flex-1 overflow-y-auto">
                {move || {
                    if let Some(state_data) = current_state.get() {
                        view! {
                            <div class="space-y-2">
                                <div class="flex gap-4 card-secondary card-compact text-sm">
                                    <div class="font-semibold text-primary">"Node: "{state_data.node_id}</div>
                                    <div class="text-secondary">"Updated: "{format_timestamp(state_data.timestamp)}</div>
                                </div>

                                <div class="card-secondary card-compact">
                                    {match view_mode.get() {
                                        ViewMode::Tree => view! {
                                            <JsonTreeView
                                                value=state_data.state
                                                expanded_paths=expanded_paths
                                                set_expanded_paths=expanded_paths
                                                search_term=search_term.get()
                                                root_path="".to_string()
                                            />
                                        }.into_any(),
                                        ViewMode::Raw => view! {
                                            <RawJsonView
                                                value=state_data.state
                                                search_term=search_term.get()
                                            />
                                        }.into_any()
                                    }}
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        // Show appropriate message based on data source
                        let source = current_source.get();
                        let service = data_source_manager_for_view.get_service();
                        let connection_status = service.get_connection_status();

                        match source {
                            DataSource::Mock => view! {
                                <div class="card-secondary card-compact text-center">
                                    <p>"No state data available"</p>
                                    <p class="hint text-sm text-secondary">"Select a node from the network view to inspect its state"</p>
                                </div>
                            }.into_any(),
                            DataSource::Simulator => view! {
                                <div class="card-secondary card-compact text-center">
                                    {match connection_status {
                                        ConnectionState::Disconnected => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-yellow-500 rounded-full animate-pulse"></div>
                                                    <h3 class="heading-4 text-yellow-800 dark:text-yellow-200">"Simulator Not Connected"</h3>
                                                </div>
                                                <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                                                    "No state data available. The simulation server is not running."
                                                </p>
                                                <div class="text-xs text-yellow-600 dark:text-yellow-400 bg-yellow-100 dark:bg-yellow-900/40 p-3 rounded">
                                                    "Start simulation server: "<code class="font-mono">"cargo run --bin sim-server"</code>
                                                </div>
                                            </div>
                                        }.into_any(),
                                        ConnectionState::Connecting => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-blue-500 rounded-full animate-pulse"></div>
                                                    <h3 class="heading-4 text-blue-800 dark:text-blue-200">"Connecting to Simulator..."</h3>
                                                </div>
                                                <p class="text-sm text-blue-700 dark:text-blue-300">
                                                    "Attempting to connect to simulation server"
                                                </p>
                                            </div>
                                        }.into_any(),
                                        ConnectionState::Connected => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-gray-500 rounded-full"></div>
                                                    <h3 class="heading-4 text-gray-800 dark:text-gray-200">"No State Data"</h3>
                                                </div>
                                                <p class="text-sm text-gray-700 dark:text-gray-300">
                                                    "Connected to simulator but no state data available"
                                                </p>
                                            </div>
                                        }.into_any(),
                                        ConnectionState::Error(ref error) => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-red-500 rounded-full"></div>
                                                    <h3 class="heading-4 text-red-800 dark:text-red-200">"Connection Error"</h3>
                                                </div>
                                                <p class="text-sm text-red-700 dark:text-red-300 mb-2">
                                                    "Failed to connect to simulation server:"
                                                </p>
                                                <code class="text-xs text-red-600 dark:text-red-400 bg-red-100 dark:bg-red-900/40 p-2 rounded block">
                                                    {error.clone()}
                                                </code>
                                            </div>
                                        }.into_any(),
                                    }}
                                </div>
                            }.into_any(),
                            DataSource::Real => view! {
                                <div class="card-secondary card-compact text-center">
                                    {match connection_status {
                                        ConnectionState::Disconnected => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-yellow-500 rounded-full animate-pulse"></div>
                                                    <h3 class="heading-4 text-yellow-800 dark:text-yellow-200">"Live Network Not Connected"</h3>
                                                </div>
                                                <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                                                    "No state data available. No live Aura node is running with instrumentation."
                                                </p>
                                                <div class="text-xs text-yellow-600 dark:text-yellow-400 bg-yellow-100 dark:bg-yellow-900/40 p-3 rounded">
                                                    "Start instrumented node: "<code class="font-mono">"aura node --dev-console --dev-console-port 9003"</code>
                                                </div>
                                            </div>
                                        }.into_any(),
                                        ConnectionState::Connecting => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-blue-500 rounded-full animate-pulse"></div>
                                                    <h3 class="heading-4 text-blue-800 dark:text-blue-200">"Connecting to Live Network..."</h3>
                                                </div>
                                                <p class="text-sm text-blue-700 dark:text-blue-300">
                                                    "Attempting to connect to live node"
                                                </p>
                                            </div>
                                        }.into_any(),
                                        ConnectionState::Connected => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-gray-500 rounded-full"></div>
                                                    <h3 class="heading-4 text-gray-800 dark:text-gray-200">"No State Data"</h3>
                                                </div>
                                                <p class="text-sm text-gray-700 dark:text-gray-300">
                                                    "Connected to live node but no state data available"
                                                </p>
                                            </div>
                                        }.into_any(),
                                        ConnectionState::Error(ref error) => view! {
                                            <div class="p-6 text-center">
                                                <div class="flex items-center justify-center gap-3 mb-3">
                                                    <div class="w-3 h-3 bg-red-500 rounded-full"></div>
                                                    <h3 class="heading-4 text-red-800 dark:text-red-200">"Connection Error"</h3>
                                                </div>
                                                <p class="text-sm text-red-700 dark:text-red-300 mb-2">
                                                    "Failed to connect to live network node:"
                                                </p>
                                                <code class="text-xs text-red-600 dark:text-red-400 bg-red-100 dark:bg-red-900/40 p-2 rounded block">
                                                    {error.clone()}
                                                </code>
                                            </div>
                                        }.into_any(),
                                    }}
                                </div>
                            }.into_any(),
                        }
                    }
                }}
            </div>
        </div>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Tree,
    Raw,
}

#[component]
fn JsonTreeView(
    value: Value,
    expanded_paths: RwSignal<HashMap<String, bool>>,
    set_expanded_paths: RwSignal<HashMap<String, bool>>,
    search_term: String,
    root_path: String,
) -> impl IntoView {
    view! {
        <div class="json-tree">
            <JsonTreeNode
                value=value
                expanded_paths=expanded_paths
                set_expanded_paths=set_expanded_paths
                search_term=search_term
                path=root_path
                key="root".to_string()
                level=0
            />
        </div>
    }
}

#[component]
fn JsonTreeNode(
    value: Value,
    expanded_paths: RwSignal<HashMap<String, bool>>,
    set_expanded_paths: RwSignal<HashMap<String, bool>>,
    search_term: String,
    path: String,
    key: String,
    level: usize,
) -> impl IntoView {
    let full_path = if path.is_empty() {
        key.clone()
    } else {
        format!("{}.{}", path, key)
    };
    let full_path_clone = full_path.clone();
    let is_expanded = move || {
        expanded_paths
            .get()
            .get(&full_path_clone)
            .copied()
            .unwrap_or(false)
    };

    let toggle_expansion = {
        let full_path = full_path.clone();
        move |_| {
            set_expanded_paths.update(|paths| {
                let current = paths.get(&full_path).copied().unwrap_or(level < 2);
                paths.insert(full_path.clone(), !current);
            });
        }
    };

    let matches_search = if search_term.is_empty() {
        true
    } else {
        key.to_lowercase().contains(&search_term.to_lowercase())
            || value_to_string(&value)
                .to_lowercase()
                .contains(&search_term.to_lowercase())
    };

    if !matches_search {
        return ().into_any();
    }

    let indent_style = format!("margin-left: {}px", level * 20);

    match value {
        Value::Object(obj) => {
            let obj_len = obj.len();
            let obj_data: Vec<(String, Value)> = obj.into_iter().collect();

            view! {
                <div class=format!("{} {}", "tree-node", "object-node") style={indent_style}>
                    <div class="node-header flex items-center gap-1 cursor-pointer" on:click=toggle_expansion>
                        <span class="expand-icon inline-flex">
                            {
                                let is_expanded = is_expanded.clone();
                                move || if is_expanded() {
                                    view! { <ChevronDown size=14 /> }.into_any()
                                } else {
                                    view! { <ChevronRight size=14 /> }.into_any()
                                }
                            }
                        </span>
                        <span class="key">{key}</span>
                        <span class="type-hint">{"{"}{obj_len}{"}"}</span>
                    </div>
                    {
                        let is_expanded = is_expanded.clone();
                        let obj_data = obj_data.clone();
                        let search_term = search_term.clone();
                        let full_path = full_path.clone();
                        move || if is_expanded() {
                            view! {
                                <div class="node-children">
                                    <ObjectChildren
                                        obj_data=obj_data.clone()
                                        expanded_paths=expanded_paths
                                        set_expanded_paths=set_expanded_paths
                                        search_term=search_term.clone()
                                        full_path=full_path.clone()
                                        level=level + 1
                                    />
                                </div>
                            }.into_any()
                        } else {
                            ().into_any()
                        }
                    }
                </div>
            }
            .into_any()
        }
        Value::Array(arr) => {
            let arr_len = arr.len();
            let arr_data: Vec<Value> = arr.into_iter().collect();

            view! {
                <div class=format!("{} {}", "tree-node", "array-node") style={indent_style}>
                    <div class="node-header flex items-center gap-1 cursor-pointer" on:click=toggle_expansion>
                        <span class="expand-icon inline-flex">
                            {
                                let is_expanded = is_expanded.clone();
                                move || if is_expanded() {
                                    view! { <ChevronDown size=14 /> }.into_any()
                                } else {
                                    view! { <ChevronRight size=14 /> }.into_any()
                                }
                            }
                        </span>
                        <span class="key">{key}</span>
                        <span class="type-hint">{"["}{arr_len}{"]"}</span>
                    </div>
                    {
                        let is_expanded = is_expanded.clone();
                        let arr_data = arr_data.clone();
                        let search_term = search_term.clone();
                        let full_path = full_path.clone();
                        move || if is_expanded() {
                            view! {
                                <div class="node-children">
                                    <ArrayChildren
                                        arr_data=arr_data.clone()
                                        expanded_paths=expanded_paths
                                        set_expanded_paths=set_expanded_paths
                                        search_term=search_term.clone()
                                        full_path=full_path.clone()
                                        level=level + 1
                                    />
                                </div>
                            }.into_any()
                        } else {
                            ().into_any()
                        }
                    }
                </div>
            }
            .into_any()
        }
        value => {
            let value_type = get_value_type(&value);
            let value_str = value_to_string(&value);
            view! {
                <div class=format!("{} {}", "tree-node", "leaf-node") style={indent_style}>
                    <span class="key">{key}": "</span>
                    <span class=format!("{} {}", "value", value_type)>
                        {value_str}
                    </span>
                </div>
            }
            .into_any()
        }
    }
}

#[component]
fn RawJsonView(value: Value, search_term: String) -> impl IntoView {
    let formatted_json = serde_json::to_string_pretty(&value).unwrap_or_default();

    view! {
        <div class="raw-json">
            <pre class="json-content">
                {if search_term.is_empty() {
                    formatted_json
                } else {
                    highlight_search(&formatted_json, &search_term)
                }}
            </pre>
        </div>
    }
}

fn format_timestamp(timestamp: u64) -> String {
    format!("{} (timestamp)", timestamp)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{}\"", s),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => "...".to_string(),
    }
}

fn get_value_type(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
    }
}

fn highlight_search(text: &str, search_term: &str) -> String {
    if search_term.is_empty() {
        return text.to_string();
    }

    text.replace(search_term, &format!("<mark>{}</mark>", search_term))
}

#[component]
fn ObjectChildren(
    obj_data: Vec<(String, Value)>,
    expanded_paths: RwSignal<HashMap<String, bool>>,
    set_expanded_paths: RwSignal<HashMap<String, bool>>,
    search_term: String,
    full_path: String,
    level: usize,
) -> impl IntoView {
    view! {
        {
            obj_data.into_iter().map(|(child_key, child_value)| {
                view! {
                    <JsonTreeNode
                        value=child_value
                        expanded_paths=expanded_paths
                        set_expanded_paths=set_expanded_paths
                        search_term=search_term.clone()
                        path=full_path.clone()
                        key=child_key
                        level=level
                    />
                }
            }).collect::<Vec<_>>()
        }
    }
}

#[component]
fn ArrayChildren(
    arr_data: Vec<Value>,
    expanded_paths: RwSignal<HashMap<String, bool>>,
    set_expanded_paths: RwSignal<HashMap<String, bool>>,
    search_term: String,
    full_path: String,
    level: usize,
) -> impl IntoView {
    view! {
        {
            arr_data.into_iter().enumerate().map(|(index, child_value)| {
                view! {
                    <JsonTreeNode
                        value=child_value
                        expanded_paths=expanded_paths
                        set_expanded_paths=set_expanded_paths
                        search_term=search_term.clone()
                        path=full_path.clone()
                        key=format!("[{}]", index)
                        level=level
                    />
                }
            }).collect::<Vec<_>>()
        }
    }
}
