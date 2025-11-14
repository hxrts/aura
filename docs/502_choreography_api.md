# Choreography API Reference

Quick reference for choreographic programming DSL and implementation patterns. Choreographic programming specifies distributed protocols from a global perspective with automatic projection to device-specific implementations.

Choreographic programming provides deadlock freedom, type safety, implementation correctness, and privacy verification through global specification. The aura-macros system generates projected implementations from declarative specifications.

See [Advanced Choreography Guide](804_advanced_choreography_guide.md) for detailed implementation patterns. See [Core Systems Guide](802_core_systems_guide.md) for effect system integration.

---

## DSL Syntax

```rust
use aura_macros::choreography;
use aura_core::effects::{ConsoleEffects, CryptoEffects, NetworkEffects, TimeEffects, JournalEffects};

/// Sealed supertrait for protocol effects
pub trait MyProtocolEffects: ConsoleEffects + CryptoEffects + NetworkEffects + TimeEffects + JournalEffects {}
impl<T> MyProtocolEffects for T where T: ConsoleEffects + CryptoEffects + NetworkEffects + TimeEffects + JournalEffects {}

/// Simple two-party request-response choreography
choreography! {
    #[namespace = "my_protocol"]
    protocol RequestResponse {
        roles: Client, Server;

        // Client sends request to Server with capability guard and flow cost
        Client[guard_capability = "send_request",
               flow_cost = 100,
               journal_facts = "request_sent"]
        -> Server: SendRequest(RequestData);

        // Server responds with computed result
        Server[guard_capability = "send_response", 
               flow_cost = 50,
               journal_facts = "response_sent",
               leakage_budget = [0, 1, 0]]  // Allow neighbor leakage
        -> Client: SendResponse(ResponseData);
    }
}
```

Syntax elements: namespace (unique protocol ID), roles (participant types), message flow (`Role[annotations] -> Role: MessageType(MessageStruct)`), guard annotations (capability requirements, flow costs, journal facts), privacy annotations (leakage budgets).

## Message Type Definitions

Message types are standard Rust structs with Serialize/Deserialize support:

```rust
use serde::{Serialize, Deserialize};
use aura_core::{DeviceId, AccountId, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestData {
    /// Unique request identifier
    pub request_id: String,
    /// Operation to perform
    pub operation: OperationType,
    /// Request timestamp
    pub timestamp: Timestamp,
    /// Optional request payload
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseData {
    /// Request ID this response corresponds to
    pub request_id: String,
    /// Operation result
    pub result: OperationResult,
    /// Response timestamp
    pub timestamp: Timestamp,
    /// Optional response data
    pub data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    GetData { key: String },
    SetData { key: String, value: Vec<u8> },
    DeleteData { key: String },
    ListKeys { prefix: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationResult {
    Success,
    NotFound,
    PermissionDenied,
    InvalidRequest { reason: String },
    InternalError { message: String },
}
```

Message types should use descriptive field names with documentation, include appropriate metadata (IDs, timestamps), leverage Rust's type system for validation, and avoid complex nested structures.

## Protocol Implementation

Choreographies generate session functions that execute using effect handlers. Implementation follows this pattern:

```rust
/// Execute the RequestResponse protocol as Client
pub async fn execute_client_session<E: MyProtocolEffects>(
    effects: &E,
    request: RequestData,
    server_device: DeviceId,
) -> Result<ResponseData, ProtocolError> {
    // Send request to server
    let request_bytes = serde_json::to_vec(&request)
        .map_err(|e| ProtocolError::Serialization(e.to_string()))?;
    
    effects.send_to_peer(server_device.into(), request_bytes).await
        .map_err(ProtocolError::Network)?;

    // Receive response from server  
    let (peer_id, response_bytes) = effects.receive().await
        .map_err(ProtocolError::Network)?;
    
    if peer_id != server_device.into() {
        return Err(ProtocolError::UnexpectedPeer);
    }

    let response: ResponseData = serde_json::from_slice(&response_bytes)
        .map_err(|e| ProtocolError::Deserialization(e.to_string()))?;

    Ok(response)
}

/// Execute the RequestResponse protocol as Server  
pub async fn execute_server_session<E: MyProtocolEffects>(
    effects: &E,
    handler: impl RequestHandler<E>,
) -> Result<(), ProtocolError> {
    // Receive request from client
    let (client_id, request_bytes) = effects.receive().await
        .map_err(ProtocolError::Network)?;
    
    let request: RequestData = serde_json::from_slice(&request_bytes)
        .map_err(|e| ProtocolError::Deserialization(e.to_string()))?;

    // Process request using business logic
    let response = handler.handle_request(request, effects).await?;

    // Send response back to client
    let response_bytes = serde_json::to_vec(&response)
        .map_err(|e| ProtocolError::Serialization(e.to_string()))?;
    
    effects.send_to_peer(client_id, response_bytes).await
        .map_err(ProtocolError::Network)?;

    Ok(())
}

/// Business logic trait for handling requests
#[async_trait::async_trait]
pub trait RequestHandler<E: MyProtocolEffects> {
    async fn handle_request(
        &self, 
        request: RequestData, 
        effects: &E
    ) -> Result<ResponseData, ProtocolError>;
}
```

## Sealed Supertraits Pattern

Sealed supertraits provide clean type boundaries and improved ergonomics:

```rust
/// Protocol-specific effects supertrait
pub trait ThresholdProtocolEffects: 
    ConsoleEffects + CryptoEffects + NetworkEffects + RandomEffects + TimeEffects + JournalEffects 
{
    // Protocol-specific effect extensions can go here
}

// Blanket implementation for any type satisfying the bounds
impl<T> ThresholdProtocolEffects for T 
where 
    T: ConsoleEffects + CryptoEffects + NetworkEffects + RandomEffects + TimeEffects + JournalEffects 
{}

/// Multi-party threshold signing choreography
choreography! {
    #[namespace = "threshold_signing"]
    protocol ThresholdSigning {
        roles: Coordinator, Signer1, Signer2, Signer3;

        // Coordinator initiates signing round
        Coordinator[guard_capability = "initiate_signing",
                   flow_cost = 200,
                   journal_facts = "signing_initiated"]
        -> Signer1: SigningRequest(ThresholdSigningRequest);
        
        Coordinator[guard_capability = "initiate_signing",
                   flow_cost = 200,
                   journal_facts = "signing_initiated"] 
        -> Signer2: SigningRequest(ThresholdSigningRequest);
        
        Coordinator[guard_capability = "initiate_signing",
                   flow_cost = 200,
                   journal_facts = "signing_initiated"]
        -> Signer3: SigningRequest(ThresholdSigningRequest);

        // Signers respond with signature shares
        Signer1[guard_capability = "provide_signature",
               flow_cost = 150,
               journal_facts = "signature_provided",
               leakage_budget = [1, 0, 0]]  // Limited external leakage
        -> Coordinator: SignatureShare(ThresholdSignatureShare);
        
        Signer2[guard_capability = "provide_signature",
               flow_cost = 150, 
               journal_facts = "signature_provided",
               leakage_budget = [1, 0, 0]]
        -> Coordinator: SignatureShare(ThresholdSignatureShare);
        
        Signer3[guard_capability = "provide_signature",
               flow_cost = 150,
               journal_facts = "signature_provided", 
               leakage_budget = [1, 0, 0]]
        -> Coordinator: SignatureShare(ThresholdSignatureShare);

        // Coordinator broadcasts final signature
        Coordinator[guard_capability = "finalize_signature",
                   flow_cost = 100,
                   journal_facts = "signature_finalized",
                   journal_merge = true]  // Merge journal state
        -> Signer1: SignatureComplete(FinalizedSignature);
        
        Coordinator[guard_capability = "finalize_signature",
                   flow_cost = 100,
                   journal_facts = "signature_finalized",
                   journal_merge = true]
        -> Signer2: SignatureComplete(FinalizedSignature);
        
        Coordinator[guard_capability = "finalize_signature", 
                   flow_cost = 100,
                   journal_facts = "signature_finalized",
                   journal_merge = true]
        -> Signer3: SignatureComplete(FinalizedSignature);
    }
}
```

## Enhanced Choreography Annotations

The aura-macros system supports rich annotations for integration with Aura's security and privacy systems:

### Guard Capabilities
```rust
Client[guard_capability = "execute_operation"]  // Single capability
Server[guard_capability = "admin_operations,read_data"]  // Multiple capabilities
```

### Flow Costs
```rust
Client[flow_cost = 150]  // Fixed cost
Server[flow_cost = 100]  // Cost charged to sender's flow budget
```

### Journal Integration
```rust
// Add facts to the journal
Client[journal_facts = "operation_executed"]

// Merge journal state across participants
Server[journal_merge = true]  

// Specify complex journal operations
Coordinator[journal_facts = "threshold_complete,signatures_verified"]
```

### Privacy Analysis
```rust
// Leakage budget: [external, neighbor, group]
Client[leakage_budget = [0, 1, 0]]    // Only neighbor context leakage
Server[leakage_budget = [1, 0, 0]]    // Limited external leakage
Guard[leakage_budget = [0, 0, 2]]     // Group context sharing allowed
```

## Effect System Integration

Choreographic protocols execute through the effect system using sealed supertraits:

```rust
use aura_effects::{RealCryptoHandler, RealNetworkHandler, RealTimeHandler};
use aura_protocol::{CompositeHandler, EnhancedTimeHandler};

/// Production effect handler composition
pub struct ProductionEffectHandler {
    crypto: RealCryptoHandler,
    network: RealNetworkHandler, 
    time: EnhancedTimeHandler,
    console: ConsoleHandler,
    journal: JournalHandler,
}

impl ConsoleEffects for ProductionEffectHandler {
    async fn log_info(&self, message: &str) {
        self.console.log_info(message).await
    }
    // ... other console methods
}

impl CryptoEffects for ProductionEffectHandler {
    async fn hash(&self, input: &[u8]) -> Vec<u8> {
        self.crypto.hash(input).await
    }
    // ... other crypto methods
}

impl NetworkEffects for ProductionEffectHandler {
    async fn send_to_peer(&self, peer: aura_core::PeerId, data: Vec<u8>) -> Result<(), aura_core::AuraError> {
        self.network.send_to_peer(peer, data).await
    }
    // ... other network methods
}

// Automatic implementation of sealed supertrait
impl ThresholdProtocolEffects for ProductionEffectHandler {}

/// Execute threshold signing with production handlers
pub async fn run_production_threshold_signing(
    device_id: DeviceId,
    signing_request: ThresholdSigningRequest,
) -> Result<FinalizedSignature, ThresholdSigningError> {
    let effects = ProductionEffectHandler::new(device_id)?;
    
    // Execute choreography session
    execute_coordinator_session(&effects, signing_request).await
        .map_err(ThresholdSigningError::Protocol)
}
```

## Error Handling Patterns

Robust choreographic protocols handle errors gracefully:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Network error: {0}")]
    Network(#[from] aura_core::AuraError),
    
    #[error("Serialization failed: {0}")]
    Serialization(String),
    
    #[error("Deserialization failed: {0}")]
    Deserialization(String),
    
    #[error("Timeout waiting for response")]
    Timeout,
    
    #[error("Unexpected peer: expected {expected}, got {actual}")]
    UnexpectedPeer { expected: DeviceId, actual: DeviceId },
    
    #[error("Protocol validation failed: {0}")]
    Validation(String),
    
    #[error("Guard chain denied operation: {0}")]
    GuardDenied(String),
}

/// Resilient session with timeout and retry logic
pub async fn execute_resilient_session<E: MyProtocolEffects>(
    effects: &E,
    request: RequestData,
    server_device: DeviceId,
    timeout: Duration,
    max_retries: usize,
) -> Result<ResponseData, ProtocolError> {
    for attempt in 0..=max_retries {
        match tokio::time::timeout(
            timeout,
            execute_client_session(effects, request.clone(), server_device)
        ).await {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(e)) => {
                if attempt == max_retries {
                    return Err(e);
                }
                // Log retry attempt
                effects.log_warn(&format!(
                    "Session attempt {} failed: {}, retrying...", 
                    attempt + 1, e
                )).await;
                
                // Exponential backoff
                let delay = Duration::from_millis(100 * 2_u64.pow(attempt as u32));
                tokio::time::sleep(delay).await;
            }
            Err(_) => {
                if attempt == max_retries {
                    return Err(ProtocolError::Timeout);
                }
            }
        }
    }
    
    unreachable!()
}
```

## Testing Choreographic Protocols

Test choreographic protocols using mock effect handlers:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use aura_effects::{MockCryptoHandler, MockNetworkHandler};
    use tokio::sync::mpsc;
    
    /// Mock effect handler for testing
    struct TestEffectHandler {
        network_tx: mpsc::UnboundedSender<(aura_core::PeerId, Vec<u8>)>,
        network_rx: std::sync::Mutex<mpsc::UnboundedReceiver<(aura_core::PeerId, Vec<u8>)>>,
    }
    
    impl ConsoleEffects for TestEffectHandler {
        async fn log_info(&self, _message: &str) {}
        async fn log_warn(&self, _message: &str) {}
        async fn log_error(&self, _message: &str) {}
        async fn log_debug(&self, _message: &str) {}
    }
    
    impl NetworkEffects for TestEffectHandler {
        async fn send_to_peer(&self, peer: aura_core::PeerId, data: Vec<u8>) -> Result<(), aura_core::AuraError> {
            self.network_tx.send((peer, data)).map_err(|_| aura_core::AuraError::network("Send failed"))
        }
        
        async fn receive(&self) -> Result<(aura_core::PeerId, Vec<u8>), aura_core::AuraError> {
            let mut rx = self.network_rx.lock().unwrap();
            rx.recv().await.ok_or_else(|| aura_core::AuraError::network("Receive failed"))
        }
    }
    
    impl CryptoEffects for TestEffectHandler {
        async fn hash(&self, input: &[u8]) -> Vec<u8> {
            aura_core::hash::hash(input).to_vec()
        }
        
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![0u8; len] // Deterministic for tests
        }
        
        async fn ed25519_sign(&self, _key: &[u8], message: &[u8]) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(aura_core::hash::hash(message).to_vec())
        }
        
        async fn ed25519_verify(&self, _public_key: &[u8], _message: &[u8], _signature: &[u8]) -> Result<bool, aura_core::AuraError> {
            Ok(true)
        }
    }
    
    impl TimeEffects for TestEffectHandler {
        async fn current_timestamp(&self) -> u64 {
            1234567890
        }
        
        async fn sleep(&self, _duration: std::time::Duration) {}
    }
    
    impl JournalEffects for TestEffectHandler {}
    
    // Automatic sealed supertrait implementation
    impl MyProtocolEffects for TestEffectHandler {}
    
    #[tokio::test]
    async fn test_request_response_success() {
        let (tx, rx) = mpsc::unbounded_channel();
        let client_effects = TestEffectHandler {
            network_tx: tx.clone(),
            network_rx: std::sync::Mutex::new(rx),
        };
        
        let request = RequestData {
            request_id: "test_001".to_string(),
            operation: OperationType::GetData { key: "test_key".to_string() },
            timestamp: 1234567890,
            payload: None,
        };
        
        let server_device = DeviceId::new();
        
        // This would typically run client and server sessions concurrently
        // For this test, we'll just verify the protocol structure
        let result = execute_client_session(&client_effects, request, server_device).await;
        assert!(result.is_ok() || matches!(result, Err(ProtocolError::Network(_))));
    }
    
    #[tokio::test]
    async fn test_protocol_timeout() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx); // Close receiver to simulate timeout
        
        let client_effects = TestEffectHandler {
            network_tx: tx,
            network_rx: std::sync::Mutex::new(mpsc::unbounded_channel().1),
        };
        
        let request = RequestData {
            request_id: "timeout_test".to_string(),
            operation: OperationType::GetData { key: "test".to_string() },
            timestamp: 1234567890,
            payload: None,
        };
        
        let result = execute_resilient_session(
            &client_effects,
            request,
            DeviceId::new(),
            Duration::from_millis(100),
            0, // No retries
        ).await;
        
        assert!(matches!(result, Err(ProtocolError::Timeout)));
    }
}
```

## Production Deployment

Deploy choreographic protocols in production using proper effect handler composition:

```rust
use aura_effects::*;
use aura_protocol::{CompositeHandler, EnhancedTimeHandler, GuardedJournalHandler};

/// Production choreography runner
pub struct ChoreographyRunner<E: MyProtocolEffects> {
    effects: E,
    device_id: DeviceId,
    config: ChoreographyConfig,
}

impl<E: MyProtocolEffects> ChoreographyRunner<E> {
    pub fn new(effects: E, device_id: DeviceId, config: ChoreographyConfig) -> Self {
        Self { effects, device_id, config }
    }
    
    /// Execute protocol with proper error handling and monitoring
    pub async fn execute_protocol<T>(&self, session: impl ProtocolSession<E, T>) -> Result<T, ProtocolError> {
        let start_time = self.effects.current_timestamp().await;
        
        let result = tokio::time::timeout(
            self.config.protocol_timeout,
            session.execute(&self.effects)
        ).await;
        
        let duration = self.effects.current_timestamp().await - start_time;
        
        match result {
            Ok(Ok(value)) => {
                self.effects.log_info(&format!(
                    "Protocol completed successfully in {}ms", duration
                )).await;
                Ok(value)
            }
            Ok(Err(e)) => {
                self.effects.log_error(&format!(
                    "Protocol failed after {}ms: {}", duration, e
                )).await;
                Err(e)
            }
            Err(_) => {
                self.effects.log_error(&format!(
                    "Protocol timed out after {}ms", duration  
                )).await;
                Err(ProtocolError::Timeout)
            }
        }
    }
}

/// Protocol session trait for type-safe execution
#[async_trait::async_trait]
pub trait ProtocolSession<E: MyProtocolEffects, T> {
    async fn execute(&self, effects: &E) -> Result<T, ProtocolError>;
}

/// Configuration for choreography execution
#[derive(Debug, Clone)]
pub struct ChoreographyConfig {
    pub protocol_timeout: Duration,
    pub retry_attempts: usize,
    pub backoff_multiplier: f64,
    pub enable_monitoring: bool,
}

impl Default for ChoreographyConfig {
    fn default() -> Self {
        Self {
            protocol_timeout: Duration::from_secs(30),
            retry_attempts: 3,
            backoff_multiplier: 2.0,
            enable_monitoring: true,
        }
    }
}
```

## Best Practices

1. **Use Sealed Supertraits**: Always define protocol-specific sealed supertraits for cleaner type signatures and better error messages.

2. **Leverage Effect System**: Use appropriate effect handlers for different environments (testing, simulation, production).

3. **Handle Errors Gracefully**: Implement comprehensive error handling with timeouts, retries, and clear error messages.

4. **Design for Privacy**: Use leakage budgets and journal annotations to maintain context isolation.

5. **Test Thoroughly**: Write comprehensive tests covering happy paths, error conditions, and edge cases.

6. **Document Protocols**: Include clear documentation explaining protocol purpose, message flows, and security properties.

7. **Monitor Production**: Implement monitoring and logging for production protocol execution.

8. **Version Carefully**: Use semantic versioning for protocol changes and maintain backward compatibility.

This choreographic programming system provides a powerful foundation for implementing secure, privacy-preserving distributed protocols in Aura while maintaining type safety and integration with the broader effect system architecture.
