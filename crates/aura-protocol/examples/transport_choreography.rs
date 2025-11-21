//! Transport Choreography Examples
//!
//! This example demonstrates how to use choreographic protocols for
//! complex multi-party transport coordination using aura-macros and
//! rumpsteak-aura integration.
//!
//! Key concepts:
//! - Multi-party coordination using choreographic protocols
//! - Session type safety for complex interaction patterns
//! - Guard capabilities, flow costs, and journal facts
//! - Extension effects through aura-macros and AuraRuntime

use aura_core::{identifiers::DeviceId, ContextId};
use aura_macros::choreography;
use aura_protocol::transport::channel_management::{
    AllocatedResources, ChannelConfirmation, ChannelType, ConfirmationResult,
};
use aura_protocol::transport::websocket::{
    WebSocketHandshakeInit, WebSocketHandshakeResponse, WebSocketHandshakeResult,
};
use aura_protocol::transport::{
    ChannelEstablishmentCoordinator, ChoreographicConfig, WebSocketHandshakeCoordinator,
};
use std::collections::HashMap;
use std::time::SystemTime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Transport Choreography Examples ===\n");

    // Example 1: WebSocket handshake choreography
    websocket_handshake_example().await?;

    // Example 2: Channel establishment choreography
    channel_establishment_example().await?;

    // Example 3: Custom choreographic transport protocol
    custom_protocol_example().await?;

    Ok(())
}

/// Example 1: WebSocket handshake choreography
/// Shows multi-party WebSocket coordination with capability negotiation
async fn websocket_handshake_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤝 Example 1: WebSocket Handshake Choreography\n");

    let initiator_id = DeviceId::from_bytes([1u8; 32]);
    let responder_id = DeviceId::from_bytes([2u8; 32]);
    let context_id = ContextId::new();

    // Create choreographic configuration
    let choreo_config = ChoreographicConfig {
        execution_timeout: std::time::Duration::from_secs(30),
        max_concurrent_protocols: 10,
        default_flow_budget: 1000,
        required_capabilities: vec![
            "websocket_handshake".to_string(),
            "capability_negotiation".to_string(),
        ],
    };

    // Create coordinators for both roles
    let mut initiator_coordinator =
        WebSocketHandshakeCoordinator::new(initiator_id, choreo_config.clone());

    let mut responder_coordinator = WebSocketHandshakeCoordinator::new(responder_id, choreo_config);

    println!("Created choreographic coordinators:");
    println!("  Initiator: {:?}", &initiator_id.to_bytes().unwrap()[..4]);
    println!("  Responder: {:?}", &responder_id.to_bytes().unwrap()[..4]);

    // Initiate handshake (choreographic coordination)
    let session_id = initiator_coordinator.initiate_handshake(
        responder_id,
        "wss://relay.example.com/handshake".to_string(),
        context_id,
    )?;

    println!("\nHandshake initiated with session: {}", &session_id[..16]);

    // Simulate choreographic message exchange
    let handshake_init = WebSocketHandshakeInit {
        session_id: session_id.clone(),
        initiator_id,
        websocket_url: "wss://relay.example.com/handshake".to_string(),
        supported_protocols: vec!["aura-v1".to_string()],
        capabilities: vec!["secure_messaging".to_string()],
        context_id,
    };

    println!("Choreographic handshake message:");
    println!("  Session: {}", &handshake_init.session_id[..16]);
    println!("  Protocols: {:?}", handshake_init.supported_protocols);
    println!("  Capabilities: {:?}", handshake_init.capabilities);

    // Simulate successful handshake response
    let handshake_response = WebSocketHandshakeResponse {
        session_id: session_id.clone(),
        responder_id,
        accepted_protocols: vec!["aura-v1".to_string()],
        granted_capabilities: vec!["secure_messaging".to_string()],
        handshake_result: WebSocketHandshakeResult::Success,
    };

    // Process response through choreographic coordinator
    let success = initiator_coordinator.process_handshake_response(&handshake_response)?;

    println!("\nHandshake completed successfully: {}", success);
    println!("Multi-party WebSocket coordination with session type safety");
    println!("Capability negotiation integrated into choreographic protocol\n");

    Ok(())
}

/// Example 2: Channel establishment choreography
/// Shows complex multi-phase channel setup with resource allocation
async fn channel_establishment_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔗 Example 2: Channel Establishment Choreography\n");

    let coordinator_id = DeviceId::from_bytes([10u8; 32]);
    let participant1_id = DeviceId::from_bytes([11u8; 32]);
    let participant2_id = DeviceId::from_bytes([12u8; 32]);
    let context_id = ContextId::new();

    let choreo_config = ChoreographicConfig {
        execution_timeout: std::time::Duration::from_secs(60),
        max_concurrent_protocols: 5,
        default_flow_budget: 2000,
        required_capabilities: vec![
            "channel_establishment".to_string(),
            "resource_allocation".to_string(),
        ],
    };

    let mut coordinator = ChannelEstablishmentCoordinator::new(coordinator_id, choreo_config);

    println!("Channel establishment coordinator created");
    println!(
        "  Coordinator: {:?}",
        &coordinator_id.to_bytes().unwrap()[..4]
    );
    println!("  Participants: {} peers", 2);

    // Initiate choreographic channel establishment
    let channel_id = coordinator.initiate_establishment(
        vec![participant1_id, participant2_id],
        ChannelType::SecureMessaging,
        context_id,
    )?;

    println!("\nMulti-phase channel establishment initiated:");
    println!("  Channel ID: {}", &channel_id[..16]);

    // Simulate choreographic confirmations from participants
    let confirmation1 = ChannelConfirmation {
        channel_id: channel_id.clone(),
        participant_id: participant1_id,
        confirmation_result: ConfirmationResult::Confirmed,
        allocated_resources: AllocatedResources {
            bandwidth_allocated: 100,
            storage_allocated: 1024,
            cpu_allocated: 2,
            memory_allocated: 512,
        },
        timestamp: SystemTime::now(),
    };

    let confirmation2 = ChannelConfirmation {
        channel_id: channel_id.clone(),
        participant_id: participant2_id,
        confirmation_result: ConfirmationResult::Confirmed,
        allocated_resources: AllocatedResources {
            bandwidth_allocated: 100,
            storage_allocated: 1024,
            cpu_allocated: 1,
            memory_allocated: 256,
        },
        timestamp: SystemTime::now(),
    };

    // Process confirmations through choreographic coordinator
    let ready1 = coordinator.process_confirmation(confirmation1)?;
    let ready2 = coordinator.process_confirmation(confirmation2)?;

    println!("Choreographic confirmations processed:");
    println!("  Participant 1 confirmed: {}", ready1);
    println!("  Participant 2 confirmed: {}", ready2);
    println!("  All participants ready: {}", ready1 && ready2);

    println!("\nComplex multi-phase choreographic coordination");
    println!("Resource allocation integrated into protocol flow");
    println!("Session type safety prevents coordination errors\n");

    Ok(())
}

/// Example 3: Custom choreographic transport protocol
/// Shows how to define custom choreographic protocols using aura-macros
async fn custom_protocol_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎭 Example 3: Custom Choreographic Transport Protocol\n");

    println!("Defining custom choreographic protocol with aura-macros:");
    println!("```rust");
    println!("choreography! {{");
    println!("    #[namespace = \"secure_file_transfer\"]");
    println!("    protocol SecureFileTransfer {{");
    println!("        roles: Sender, Receiver, Relay;");
    println!("        ");
    println!("        // Phase 1: Negotiate transfer parameters");
    println!("        Sender[guard_capability = \"initiate_file_transfer\",");
    println!("               flow_cost = 300,");
    println!("               journal_facts = \"transfer_initiated\"]");
    println!("        -> Receiver: TransferRequest(FileTransferRequest);");
    println!("        ");
    println!("        // Phase 2: Establish secure channel through relay");
    println!("        Receiver[guard_capability = \"setup_secure_channel\",");
    println!("                flow_cost = 200]");
    println!("        -> Relay: ChannelSetup(SecureChannelRequest);");
    println!("        ");
    println!("        // Phase 3: Confirm transfer readiness");
    println!("        Relay[guard_capability = \"confirm_channel_ready\",");
    println!("              flow_cost = 150,");
    println!("              journal_facts = \"secure_channel_established\"]");
    println!("        -> Sender: ChannelReady(ChannelConfirmation);");
    println!("    }}");
    println!("}}");
    println!("```\n");

    println!("Key choreographic features demonstrated:");
    println!("Guard capabilities: Capability-based authorization for each step");
    println!("Flow costs: Privacy budget tracking for spam prevention");
    println!("Journal facts: Distributed state synchronization");
    println!("Session types: Compile-time protocol correctness");
    println!("Multi-party coordination: Three-party interaction pattern");

    println!("\nExtension effects integration:");
    println!("- CapabilityGuardEffect: Verifies required capabilities");
    println!("- FlowCostEffect: Tracks and enforces flow budgets");
    println!("- JournalFactsEffect: Updates distributed journal state");
    println!("- LeakageBudgetEffect: Prevents information leakage");

    println!("\nRuntime integration with aura-mpst:");
    println!("- ExtensionRegistry manages Aura-specific effects");
    println!("- AuraRuntime provides session type execution");
    println!("- Guard chains enforce security policies");
    println!("- Journal coupling maintains consistency");

    println!("\nCustom protocols integrate seamlessly with Aura's choreographic system");
    println!("Session type safety ensures protocol correctness");
    println!("Extension effects provide domain-specific functionality\n");

    Ok(())
}

// Example choreographic protocol definition using aura-macros
// This shows the actual choreography! macro usage for custom protocols
choreography! {
    #[namespace = "example_secure_transport"]
    protocol ExampleSecureTransport {
        roles: Client, Server, Witness;

        // Phase 1: Client initiates secure connection
        Client[guard_capability = "initiate_secure_connection",
               flow_cost = 250,
               journal_facts = "secure_connection_initiated"]
        -> Server: SecureConnectionRequest(SecureConnectionRequest);

        // Phase 2: Server authenticates with witness
        Server[guard_capability = "authenticate_with_witness",
               flow_cost = 200,
               journal_facts = "authentication_requested"]
        -> Witness: AuthenticationRequest(AuthenticationRequest);

        // Phase 3: Witness confirms authentication
        Witness[guard_capability = "confirm_authentication",
                flow_cost = 150,
                journal_facts = "authentication_confirmed"]
        -> Server: AuthenticationResponse(AuthenticationResponse);

        // Phase 4: Server responds to client
        Server[guard_capability = "establish_secure_connection",
               flow_cost = 200,
               journal_facts = "secure_connection_established"]
        -> Client: SecureConnectionResponse(SecureConnectionResponse);
    }
}

// Message types for the choreographic protocol
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SecureConnectionRequest {
    pub client_id: DeviceId,
    pub connection_type: String,
    pub security_requirements: Vec<String>,
    pub context_id: ContextId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthenticationRequest {
    pub server_id: DeviceId,
    pub client_id: DeviceId,
    pub challenge: Vec<u8>,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthenticationResponse {
    pub witness_id: DeviceId,
    pub authentication_result: bool,
    pub trust_score: u8,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SecureConnectionResponse {
    pub server_id: DeviceId,
    pub connection_accepted: bool,
    pub session_key: Vec<u8>,
    pub connection_metadata: HashMap<String, String>,
}
