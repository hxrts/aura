/// Mock Data Service
///
/// Consolidated mock data for all console UI components.
/// Used for testing and development when WebSocket is not available.
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Branch Management Mock Data
// ============================================================================

#[derive(Clone, Debug)]
pub struct Branch {
    pub id: String,
    pub name: String,
    pub is_current: bool,
    pub parent: Option<String>,
    pub fork_tick: Option<u64>,
    pub scenario: Option<String>,
    pub last_modified: String,
    pub commit_count: u32,
}

pub fn get_mock_branches() -> Vec<Branch> {
    vec![
        Branch {
            id: "main".to_string(),
            name: "main".to_string(),
            is_current: true,
            parent: None,
            fork_tick: None,
            scenario: Some("dkd-basic.toml".to_string()),
            last_modified: "2 hours ago".to_string(),
            commit_count: 15,
        },
        Branch {
            id: "experiment-1".to_string(),
            name: "experiment-1".to_string(),
            is_current: false,
            parent: Some("main".to_string()),
            fork_tick: Some(100),
            scenario: None,
            last_modified: "30 minutes ago".to_string(),
            commit_count: 3,
        },
        Branch {
            id: "byzantine-test".to_string(),
            name: "byzantine-test".to_string(),
            is_current: false,
            parent: Some("main".to_string()),
            fork_tick: Some(50),
            scenario: Some("byzantine-scenario.toml".to_string()),
            last_modified: "1 hour ago".to_string(),
            commit_count: 7,
        },
    ]
}

// ============================================================================
// State Inspector Mock Data
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateData {
    pub node_id: String,
    pub timestamp: u64,
    pub state: Value,
}

pub fn get_mock_state_data() -> StateData {
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

// ============================================================================
// Network Visualization Mock Data
// ============================================================================

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

pub fn get_mock_network_data() -> (Vec<NetworkNode>, Vec<NetworkEdge>) {
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
