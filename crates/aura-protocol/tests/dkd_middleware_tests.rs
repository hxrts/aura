//! DKD Middleware Tests
//!
//! Comprehensive test suite for DKD using the AuraProtocolHandler middleware pattern.

use aura_crypto::Effects;
use aura_journal::{AccountLedger, Event, EventMetadata};
use aura_protocol::{
    handlers::InMemoryHandler,
    middleware::{
        AuraProtocolHandler, CapabilityMiddleware, ErrorRecoveryMiddleware, MetricsMiddleware,
        ProtocolError, ProtocolResult, SessionMiddleware, TracingMiddleware,
    },
};
use aura_types::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// DKD message types for middleware testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkdMessage {
    InitRequest {
        device_id: DeviceId,
        app_id: String,
        context: String,
    },
    Response {
        success: bool,
        error: Option<String>,
    },
    KeyDerivationRequest {
        app_id: String,
        context: String,
        threshold: usize,
    },
    KeyDerivationResponse {
        derived_key: Vec<u8>,
        proof: Vec<u8>,
    },
}

/// DKD protocol handler implementation for testing
pub struct DkdProtocolHandler {
    device_id: DeviceId,
    sessions: HashMap<Uuid, DkdSessionState>,
    message_queue: HashMap<DeviceId, Vec<DkdMessage>>,
    derived_keys: HashMap<String, Vec<u8>>, // app_id:context -> key
}

/// DKD session state
#[derive(Debug, Clone)]
pub struct DkdSessionState {
    pub session_id: Uuid,
    pub participants: Vec<DeviceId>,
    pub app_id: String,
    pub context: String,
    pub threshold: usize,
    pub started_at: u64,
    pub state: DkdState,
}

#[derive(Debug, Clone)]
pub enum DkdState {
    Initializing,
    KeyDerivation,
    Completed,
    Failed(String),
}

impl DkdProtocolHandler {
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            sessions: HashMap::new(),
            message_queue: HashMap::new(),
            derived_keys: HashMap::new(),
        }
    }

    /// Simulate key derivation for testing
    async fn derive_key(&mut self, app_id: &str, context: &str) -> Result<Vec<u8>, String> {
        let key_material = format!("{}:{}", app_id, context);
        let derived_key = blake3::hash(key_material.as_bytes()).as_bytes().to_vec();

        let key_id = format!("{}:{}", app_id, context);
        self.derived_keys.insert(key_id, derived_key.clone());

        Ok(derived_key)
    }
}

#[async_trait::async_trait]
impl AuraProtocolHandler for DkdProtocolHandler {
    type DeviceId = DeviceId;
    type SessionId = Uuid;
    type Message = DkdMessage;

    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
        self.message_queue
            .entry(to)
            .or_insert_with(Vec::new)
            .push(msg);
        Ok(())
    }

    async fn receive_message(&mut self, from: Self::DeviceId) -> ProtocolResult<Self::Message> {
        if let Some(messages) = self.message_queue.get_mut(&from) {
            if let Some(message) = messages.pop() {
                return Ok(message);
            }
        }

        // Return default response if no messages in queue
        Ok(DkdMessage::Response {
            success: false,
            error: Some("No messages available".to_string()),
        })
    }

    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: HashMap<String, String>,
    ) -> ProtocolResult<Self::SessionId> {
        let session_id = Uuid::new_v4();

        let app_id = metadata
            .get("app_id")
            .cloned()
            .unwrap_or_else(|| "default_app".to_string());
        let context = metadata
            .get("context")
            .cloned()
            .unwrap_or_else(|| "default_context".to_string());
        let threshold = metadata
            .get("threshold")
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);

        let session = DkdSessionState {
            session_id,
            participants,
            app_id,
            context,
            threshold,
            started_at: current_timestamp,
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            state: DkdState::Initializing,
        };

        self.sessions.insert(session_id, session);
        Ok(session_id)
    }

    async fn end_session(&mut self, session_id: Self::SessionId) -> ProtocolResult<()> {
        self.sessions.remove(&session_id);
        Ok(())
    }

    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> ProtocolResult<aura_protocol::middleware::SessionInfo> {
        if let Some(session) = self.sessions.get(&session_id) {
            let mut metadata = HashMap::new();
            metadata.insert("app_id".to_string(), session.app_id.clone());
            metadata.insert("context".to_string(), session.context.clone());
            metadata.insert("threshold".to_string(), session.threshold.to_string());
            metadata.insert("state".to_string(), format!("{:?}", session.state));

            Ok(aura_protocol::middleware::SessionInfo {
                session_id,
                participants: session.participants.clone(),
                protocol_type: "DKD".to_string(),
                started_at: session.started_at,
                metadata,
            })
        } else {
            Err(ProtocolError::Session {
                message: format!("Session not found: {}", session_id),
            })
        }
    }

    async fn list_sessions(
        &mut self,
    ) -> ProtocolResult<Vec<aura_protocol::middleware::SessionInfo>> {
        let mut sessions = Vec::new();
        for session in self.sessions.values() {
            let mut metadata = HashMap::new();
            metadata.insert("app_id".to_string(), session.app_id.clone());
            metadata.insert("context".to_string(), session.context.clone());
            metadata.insert("threshold".to_string(), session.threshold.to_string());
            metadata.insert("state".to_string(), format!("{:?}", session.state));

            sessions.push(aura_protocol::middleware::SessionInfo {
                session_id: session.session_id,
                participants: session.participants.clone(),
                protocol_type: "DKD".to_string(),
                started_at: session.started_at,
                metadata,
            });
        }
        Ok(sessions)
    }

    async fn verify_capability(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<bool> {
        // For testing, allow DKD operations and deny others
        match (operation, resource) {
            ("derive_key", "dkd") => Ok(true),
            ("init", "dkd") => Ok(true),
            _ => Ok(false),
        }
    }

    async fn create_authorization_proof(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<Vec<u8>> {
        // Create a simple proof for testing
        let proof_data = format!("{}:{}:{:?}", operation, resource, context);
        Ok(blake3::hash(proof_data.as_bytes()).as_bytes().to_vec())
    }

    fn device_id(&self) -> Self::DeviceId {
        self.device_id
    }
}

/// Build a DKD middleware stack for testing
pub fn build_dkd_middleware_stack(
    base_handler: DkdProtocolHandler,
) -> impl AuraProtocolHandler<DeviceId = DeviceId, SessionId = Uuid, Message = DkdMessage> {
    let handler = SessionMiddleware::new(base_handler);
    let handler = CapabilityMiddleware::new(handler);
    let handler = ErrorRecoveryMiddleware::new(handler);
    let handler = MetricsMiddleware::new(handler);
    TracingMiddleware::new(handler)
}

/// Create a test context for DKD testing
async fn create_test_context(
    device_id: DeviceId,
    participants: Vec<DeviceId>,
) -> (Effects, Arc<RwLock<AccountLedger>>) {
    let effects = Effects::for_test("dkd_test");
    let ledger = Arc::new(RwLock::new(AccountLedger::new()));
    (effects, ledger)
}

#[tokio::test]
async fn test_dkd_middleware_basic() {
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();
    let device3 = DeviceId::new();
    let participants = vec![device1, device2, device3];

    // Create test contexts
    let (effects1, ledger1) = create_test_context(device1, participants.clone()).await;
    let (effects2, ledger2) = create_test_context(device2, participants.clone()).await;
    let (effects3, ledger3) = create_test_context(device3, participants.clone()).await;

    // Create handlers with middleware stacks
    let handler1 = DkdProtocolHandler::new(device1);
    let mut middleware1 = build_dkd_middleware_stack(handler1);

    let handler2 = DkdProtocolHandler::new(device2);
    let mut middleware2 = build_dkd_middleware_stack(handler2);

    let handler3 = DkdProtocolHandler::new(device3);
    let mut middleware3 = build_dkd_middleware_stack(handler3);

    // Setup handlers
    middleware1
        .setup()
        .await
        .expect("Failed to setup handler 1");
    middleware2
        .setup()
        .await
        .expect("Failed to setup handler 2");
    middleware3
        .setup()
        .await
        .expect("Failed to setup handler 3");

    println!(
        "DKD middleware test setup complete for devices: {:?}",
        participants
    );

    // Test session creation
    let mut session_metadata = HashMap::new();
    session_metadata.insert("app_id".to_string(), "test_app".to_string());
    session_metadata.insert("context".to_string(), "test_context".to_string());
    session_metadata.insert("threshold".to_string(), "2".to_string());

    let session_id1 = middleware1
        .start_session(
            participants.clone(),
            "DKD".to_string(),
            session_metadata.clone(),
        )
        .await
        .expect("Failed to start session");

    // Verify session was created
    let session_info = middleware1
        .get_session_info(session_id1)
        .await
        .expect("Failed to get session info");

    assert_eq!(session_info.participants, participants);
    assert_eq!(session_info.metadata.get("app_id").unwrap(), "test_app");
    assert_eq!(
        session_info.metadata.get("context").unwrap(),
        "test_context"
    );

    // Cleanup
    middleware1
        .end_session(session_id1)
        .await
        .expect("Failed to end session");
    middleware1
        .teardown()
        .await
        .expect("Failed to teardown handler 1");
    middleware2
        .teardown()
        .await
        .expect("Failed to teardown handler 2");
    middleware3
        .teardown()
        .await
        .expect("Failed to teardown handler 3");

    println!("DKD middleware basic test completed successfully");
}

#[tokio::test]
async fn test_dkd_message_serialization() {
    use serde_json;

    let device_id = DeviceId::new();
    let app_id = "test_app".to_string();
    let context = "test_context".to_string();

    let init_msg = DkdMessage::InitRequest {
        device_id,
        app_id,
        context,
    };
    let serialized = serde_json::to_string(&init_msg).expect("Failed to serialize DkdMessage");
    let deserialized: DkdMessage =
        serde_json::from_str(&serialized).expect("Failed to deserialize DkdMessage");

    match (init_msg, deserialized) {
        (
            DkdMessage::InitRequest {
                device_id: d1,
                app_id: a1,
                context: c1,
            },
            DkdMessage::InitRequest {
                device_id: d2,
                app_id: a2,
                context: c2,
            },
        ) => {
            assert_eq!(d1, d2);
            assert_eq!(a1, a2);
            assert_eq!(c1, c2);
        }
        _ => panic!("Message types don't match after serialization"),
    }

    println!("DKD message serialization test passed");
}

#[tokio::test]
async fn test_dkd_key_derivation() {
    let device_id = DeviceId::new();
    let mut handler = DkdProtocolHandler::new(device_id);

    // Test key derivation
    let app_id = "test_app";
    let context = "test_context";

    let derived_key = handler
        .derive_key(app_id, context)
        .await
        .expect("Failed to derive key");

    assert!(!derived_key.is_empty());
    assert_eq!(derived_key.len(), 32); // Blake3 hash size

    // Test that same inputs produce same key
    let derived_key2 = handler
        .derive_key(app_id, context)
        .await
        .expect("Failed to derive key");

    assert_eq!(derived_key, derived_key2);

    // Test that different inputs produce different keys
    let derived_key3 = handler
        .derive_key(app_id, "different_context")
        .await
        .expect("Failed to derive key");

    assert_ne!(derived_key, derived_key3);

    println!("DKD key derivation test passed");
}

#[tokio::test]
async fn test_dkd_message_flow() {
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();

    let handler1 = DkdProtocolHandler::new(device1);
    let mut middleware1 = build_dkd_middleware_stack(handler1);

    let handler2 = DkdProtocolHandler::new(device2);
    let mut middleware2 = build_dkd_middleware_stack(handler2);

    // Setup handlers
    middleware1
        .setup()
        .await
        .expect("Failed to setup handler 1");
    middleware2
        .setup()
        .await
        .expect("Failed to setup handler 2");

    // Device 1 sends init request to Device 2
    let init_msg = DkdMessage::InitRequest {
        device_id: device1,
        app_id: "test_app".to_string(),
        context: "test_context".to_string(),
    };

    middleware1
        .send_message(device2, init_msg.clone())
        .await
        .expect("Failed to send init message");

    // Device 2 receives init request
    let received_msg = middleware1
        .receive_message(device2)
        .await
        .expect("Failed to receive message");

    match (init_msg, received_msg) {
        (
            DkdMessage::InitRequest {
                device_id: d1,
                app_id: a1,
                context: c1,
            },
            DkdMessage::InitRequest {
                device_id: d2,
                app_id: a2,
                context: c2,
            },
        ) => {
            assert_eq!(d1, d2);
            assert_eq!(a1, a2);
            assert_eq!(c1, c2);
        }
        _ => panic!("Messages don't match"),
    }

    // Test key derivation message flow
    let key_req = DkdMessage::KeyDerivationRequest {
        app_id: "test_app".to_string(),
        context: "test_context".to_string(),
        threshold: 2,
    };

    middleware2
        .send_message(device1, key_req)
        .await
        .expect("Failed to send key derivation request");

    let key_resp_msg = middleware2
        .receive_message(device1)
        .await
        .expect("Failed to receive key derivation request");

    match key_resp_msg {
        DkdMessage::KeyDerivationRequest {
            app_id,
            context,
            threshold,
        } => {
            assert_eq!(app_id, "test_app");
            assert_eq!(context, "test_context");
            assert_eq!(threshold, 2);
        }
        _ => panic!("Expected key derivation request"),
    }

    // Cleanup
    middleware1
        .teardown()
        .await
        .expect("Failed to teardown handler 1");
    middleware2
        .teardown()
        .await
        .expect("Failed to teardown handler 2");

    println!("DKD message flow test passed");
}

#[tokio::test]
async fn test_dkd_capability_verification() {
    let device_id = DeviceId::new();
    let handler = DkdProtocolHandler::new(device_id);
    let mut middleware = build_dkd_middleware_stack(handler);

    middleware.setup().await.expect("Failed to setup handler");

    // Test allowed operations
    let can_derive = middleware
        .verify_capability("derive_key", "dkd", HashMap::new())
        .await
        .expect("Failed to verify capability");
    assert!(can_derive);

    let can_init = middleware
        .verify_capability("init", "dkd", HashMap::new())
        .await
        .expect("Failed to verify capability");
    assert!(can_init);

    // Test disallowed operations
    let can_delete = middleware
        .verify_capability("delete", "database", HashMap::new())
        .await
        .expect("Failed to verify capability");
    assert!(!can_delete);

    middleware
        .teardown()
        .await
        .expect("Failed to teardown handler");

    println!("DKD capability verification test passed");
}

#[tokio::test]
async fn test_effects_determinism() {
    let effects1 = Effects::for_test("test_seed");
    let effects2 = Effects::for_test("test_seed");

    // Test that same seed produces same results
    let timestamp1 = effects1.time.current_timestamp().unwrap_or(0);
    let timestamp2 = effects2.time.current_timestamp().unwrap_or(0);

    // In deterministic mode, timestamps should be the same
    assert_eq!(timestamp1, timestamp2);

    println!(
        "Deterministic effects test passed: {} == {}",
        timestamp1, timestamp2
    );
}

#[tokio::test]
async fn test_middleware_stack_health() {
    let device_id = DeviceId::new();
    let handler = DkdProtocolHandler::new(device_id);
    let mut middleware = build_dkd_middleware_stack(handler);

    // Test setup
    middleware
        .setup()
        .await
        .expect("Failed to setup middleware");

    // Test health check
    let is_healthy = middleware
        .health_check()
        .await
        .expect("Failed to perform health check");
    assert!(is_healthy);

    // Test authorization proof creation
    let mut context = HashMap::new();
    context.insert("session_id".to_string(), "test-session".to_string());

    let proof = middleware
        .create_authorization_proof("derive_key", "dkd", context)
        .await
        .expect("Failed to create authorization proof");

    assert!(!proof.is_empty());
    assert_eq!(proof.len(), 32); // Blake3 hash size

    // Test teardown
    middleware
        .teardown()
        .await
        .expect("Failed to teardown middleware");

    println!("Middleware stack health test passed");
}
