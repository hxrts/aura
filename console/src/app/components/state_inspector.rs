use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use stylance::import_style;

import_style!(style, "../../../styles/state-inspector.css");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateData {
    pub node_id: String,
    pub timestamp: u64,
    pub state: Value,
}

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
    let (current_state, set_current_state) = signal(None::<StateData>);
    let (expanded_paths, set_expanded_paths) = signal(HashMap::<String, bool>::new());
    let (search_term, set_search_term) = signal(String::new());
    let (view_mode, set_view_mode) = signal(ViewMode::Tree);

    let websocket_responses = use_context::<ReadSignal<VecDeque<serde_json::Value>>>()
        .unwrap_or_else(|| signal(VecDeque::new()).0);

    Effect::new(move |_| {
        let responses = websocket_responses.get();

        for response in responses.iter() {
            if let Ok(state_data) = serde_json::from_value::<StateData>(response.clone()) {
                set_current_state.set(Some(state_data));
                return;
            }
        }

        if responses.is_empty() {
            let mock_state_data = get_mock_state_data();
            set_current_state.set(Some(mock_state_data));
        }
    });

    Effect::new(move |_| {
        let mock_state = serde_json::json!({
            "device_id": "alice",
            "epoch": 42,
            "status": "online",
            "keys": {
                "root_key": {
                    "threshold": 2,
                    "participants": ["alice", "bob", "charlie"],
                    "key_id": "0x1234567890abcdef"
                },
                "session_keys": {
                    "current": "0xabcdef1234567890",
                    "previous": "0x9876543210fedcba"
                }
            },
            "ledger": {
                "latest_event": {
                    "id": 123,
                    "type": "KeyGeneration",
                    "timestamp": 1635724800,
                    "data": {
                        "participants": ["alice", "bob"],
                        "threshold": 2
                    }
                },
                "event_count": 456,
                "consensus_state": "active"
            },
            "network": {
                "peers": [
                    {
                        "id": "bob",
                        "status": "connected",
                        "last_seen": 1635724795
                    },
                    {
                        "id": "charlie",
                        "status": "disconnected",
                        "last_seen": 1635724700
                    }
                ],
                "transport": {
                    "listening_on": "0.0.0.0:8080",
                    "protocol_version": "1.0"
                }
            },
            "capabilities": [
                "threshold_signing",
                "key_derivation",
                "session_management"
            ],
            "metrics": {
                "uptime": 3600,
                "messages_sent": 142,
                "messages_received": 158,
                "errors": 3
            }
        });

        let state_data = StateData {
            node_id: "alice".to_string(),
            timestamp: 1635724800,
            state: mock_state,
        };

        set_current_state.set(Some(state_data));
    });

    view! {
        <div class=style::state_inspector_container>
            <div class=style::inspector_header>
                <h3>"State Inspector"</h3>
                <div class=style::inspector_controls>
                    <div class=style::view_mode_switcher>
                        <button
                            class=move || if view_mode.get() == ViewMode::Tree {
                                format!("{} {}", style::mode_btn, style::active)
                            } else {
                                style::mode_btn.to_string()
                            }
                            on:click=move |_| set_view_mode.set(ViewMode::Tree)
                        >
                            "Tree"
                        </button>
                        <button
                            class=move || if view_mode.get() == ViewMode::Raw {
                                format!("{} {}", style::mode_btn, style::active)
                            } else {
                                style::mode_btn.to_string()
                            }
                            on:click=move |_| set_view_mode.set(ViewMode::Raw)
                        >
                            "Raw"
                        </button>
                    </div>
                    <input
                        type="text"
                        placeholder="Search..."
                        class=style::search_input
                        prop:value=move || search_term.get()
                        on:input=move |ev| {
                            set_search_term.set(event_target_value(&ev));
                        }
                    />
                </div>
            </div>

            <div class=style::inspector_content>
                {move || {
                    if let Some(state_data) = current_state.get() {
                        view! {
                            <div class=style::state_info>
                                <div class=style::state_meta>
                                    <span class=style::node_id>{"Node: "}{state_data.node_id}</span>
                                    <span class=style::timestamp>{"Updated: "}{format_timestamp(state_data.timestamp)}</span>
                                </div>

                                <div class=style::state_content>
                                    {match view_mode.get() {
                                        ViewMode::Tree => view! {
                                            <JsonTreeView
                                                value=state_data.state
                                                expanded_paths=expanded_paths.into()
                                                set_expanded_paths=set_expanded_paths
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
                        view! {
                            <div class=style::no_state>
                                <p>"No state data available"</p>
                                <p class=style::hint>"Select a node from the network view to inspect its state"</p>
                            </div>
                        }.into_any()
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
    expanded_paths: Signal<HashMap<String, bool>>,
    set_expanded_paths: WriteSignal<HashMap<String, bool>>,
    search_term: String,
    root_path: String,
) -> impl IntoView {
    view! {
        <div class=style::json_tree>
            <JsonTreeNode
                value=value
                expanded_paths=expanded_paths.into()
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
    expanded_paths: Signal<HashMap<String, bool>>,
    set_expanded_paths: WriteSignal<HashMap<String, bool>>,
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
            .unwrap_or(level < 2)
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
        return view! {}.into_any();
    }

    let indent_style = format!("margin-left: {}px", level * 20);

    match value {
        Value::Object(obj) => {
            let obj_len = obj.len();
            let obj_data: Vec<(String, Value)> = obj.into_iter().collect();

            view! {
                <div class=format!("{} {}", style::tree_node, style::object_node) style={indent_style}>
                    <div class=style::node_header on:click=toggle_expansion>
                        <span class=style::expand_icon>
                            {
                                let is_expanded = is_expanded.clone();
                                move || if is_expanded() { "▼" } else { "▶" }
                            }
                        </span>
                        <span class=style::key>{key}</span>
                        <span class=style::type_hint>{"{"}{obj_len}{"}"}</span>
                    </div>
                    {
                        let is_expanded = is_expanded.clone();
                        let obj_data = obj_data.clone();
                        let expanded_paths = expanded_paths.clone();
                        let set_expanded_paths = set_expanded_paths.clone();
                        let search_term = search_term.clone();
                        let full_path = full_path.clone();
                        move || if is_expanded() {
                            view! {
                                <div class=style::node_children>
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
                            view! {}.into_any()
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
                <div class=format!("{} {}", style::tree_node, style::array_node) style={indent_style}>
                    <div class=style::node_header on:click=toggle_expansion>
                        <span class=style::expand_icon>
                            {
                                let is_expanded = is_expanded.clone();
                                move || if is_expanded() { "▼" } else { "▶" }
                            }
                        </span>
                        <span class=style::key>{key}</span>
                        <span class=style::type_hint>{"["}{arr_len}{"]"}</span>
                    </div>
                    {
                        let is_expanded = is_expanded.clone();
                        let arr_data = arr_data.clone();
                        let expanded_paths = expanded_paths.clone();
                        let set_expanded_paths = set_expanded_paths.clone();
                        let search_term = search_term.clone();
                        let full_path = full_path.clone();
                        move || if is_expanded() {
                            view! {
                                <div class=style::node_children>
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
                            view! {}.into_any()
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
                <div class=format!("{} {}", style::tree_node, style::leaf_node) style={indent_style}>
                    <span class=style::key>{key}": "</span>
                    <span class=format!("{} {}", style::value, value_type)>
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
        <div class=style::raw_json>
            <pre class=style::json_content>
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
    expanded_paths: Signal<HashMap<String, bool>>,
    set_expanded_paths: WriteSignal<HashMap<String, bool>>,
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
                        expanded_paths=expanded_paths.into()
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
    expanded_paths: Signal<HashMap<String, bool>>,
    set_expanded_paths: WriteSignal<HashMap<String, bool>>,
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
                        expanded_paths=expanded_paths.into()
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

fn get_mock_state_data() -> StateData {
    let mock_state = serde_json::json!({
        "device_id": "alice",
        "epoch": 42,
        "status": "online",
        "keys": {
            "root_key": {
                "threshold": 2,
                "participants": ["alice", "bob", "charlie"],
                "key_id": "0x1234567890abcdef"
            },
            "session_keys": {
                "current": "0xabcdef1234567890",
                "previous": "0x9876543210fedcba"
            }
        },
        "ledger": {
            "current_height": 1337,
            "last_block_hash": "0xdeadbeefcafebabe",
            "validators": [
                {"id": "alice", "stake": 1000, "status": "active"},
                {"id": "bob", "stake": 800, "status": "active"},
                {"id": "charlie", "stake": 600, "status": "slashed"}
            ]
        },
        "network": {
            "peer_count": 12,
            "connections": [
                {"peer": "bob", "latency": 50, "status": "healthy"},
                {"peer": "charlie", "latency": 200, "status": "degraded"}
            ]
        }
    });

    StateData {
        node_id: "alice".to_string(),
        timestamp: 1234567890,
        state: mock_state,
    }
}
