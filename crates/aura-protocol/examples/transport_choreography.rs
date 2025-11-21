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

use aura_core::{ContextId, identifiers::DeviceId};
use aura_macros::choreography;
use aura_protocol::{
    prelude::*,
    transport_coordination::{
        ChannelEstablishmentCoordinator, ChoreographicConfig, ChoreographicError,
        ReceiptVerificationCoordinator, WebSocketHandshakeCoordinator,
    },
};
use aura_transport::{
    protocols::{HolePunchMessage, WebSocketMessage},
    PrivacyLevel, TransportConfig,
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

    // Example 3: Receipt verification choreography
    receipt_verification_example().await?;

    // Example 4: Custom choreographic transport protocol
    custom_protocol_example().await?;

    Ok(())
}

/// Example 1: WebSocket handshake choreography
/// Shows multi-party WebSocket coordination with capability negotiation
async fn websocket_handshake_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤝 Example 1: WebSocket Handshake Choreography\n");

    let initiator_id = DeviceId::new();
    let responder_id = DeviceId::new();
    let context_id = ContextId::new();

    // Create choreographic configuration
    let choreo_config = ChoreographicConfig {
        max_concurrent_protocols: 10,
        protocol_timeout: std::time::Duration::from_secs(30),
        required_capabilities: vec![
            "websocket_handshake".to_string(),
            "capability_negotiation".to_string(),
        ],
        extension_registry: Default::default(),
    };

    // Create coordinators for both roles
    let mut initiator_coordinator =
        WebSocketHandshakeCoordinator::new(initiator_id, choreo_config.clone());

    let mut responder_coordinator = WebSocketHandshakeCoordinator::new(responder_id, choreo_config);

    println!("Created choreographic coordinators:");
    println!("  Initiator: {}", initiator_id.to_hex()[..8].to_string());
    println!("  Responder: {}", responder_id.to_hex()[..8].to_string());

    // Initiate handshake (choreographic coordination)
    let session_id = initiator_coordinator.initiate_handshake(
        responder_id,
        "wss://relay.example.com/handshake".to_string(),
        context_id,
    )?;

    println!("\nHandshake initiated with session: {}", &session_id[..16]);

    // Simulate choreographic message exchange
    let handshake_init = aura_transport::protocols::websocket::WebSocketHandshakeInit {
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
    let handshake_response = aura_transport::protocols::websocket::WebSocketHandshakeResponse {
        session_id: session_id.clone(),
        responder_id,
        accepted_protocols: vec!["aura-v1".to_string()],
        granted_capabilities: vec!["secure_messaging".to_string()],
        handshake_result: aura_transport::protocols::websocket::WebSocketHandshakeResult::Success,
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

    let coordinator_id = DeviceId::new();
    let participant1_id = DeviceId::new();
    let participant2_id = DeviceId::new();
    let context_id = ContextId::new();

    let choreo_config = ChoreographicConfig {
        max_concurrent_protocols: 5,
        protocol_timeout: std::time::Duration::from_secs(60),
        required_capabilities: vec![
            "channel_establishment".to_string(),
            "resource_allocation".to_string(),
        ],
        extension_registry: Default::default(),
    };

    let mut coordinator = ChannelEstablishmentCoordinator::new(coordinator_id, choreo_config);

    println!("Channel establishment coordinator created");
    println!(
        "  Coordinator: {}",
        coordinator_id.to_hex()[..8].to_string()
    );
    println!("  Participants: {} peers", 2);

    // Initiate choreographic channel establishment
    let channel_id = coordinator.initiate_establishment(
        vec![participant1_id, participant2_id],
        aura_transport::protocols::websocket::ChannelType::SecureMessaging,
        context_id,
    )?;

    println!("\nMulti-phase channel establishment initiated:");
    println!("  Channel ID: {}", &channel_id[..16]);

    // Simulate choreographic confirmations from participants
    let confirmation1 = aura_transport::protocols::websocket::ChannelConfirmation {
        channel_id: channel_id.clone(),
        participant_id: participant1_id,
        confirmation_result: aura_transport::protocols::websocket::ConfirmationResult::Confirmed,
        allocated_resources: aura_transport::protocols::websocket::AllocatedResources {
            bandwidth_allocated: 100,
            storage_allocated: 1024,
            cpu_allocated: 2,
            memory_allocated: 512,
        },
        timestamp: SystemTime::now(),
    };

    let confirmation2 = aura_transport::protocols::websocket::ChannelConfirmation {
        channel_id: channel_id.clone(),
        participant_id: participant2_id,
        confirmation_result: aura_transport::protocols::websocket::ConfirmationResult::Confirmed,
        allocated_resources: aura_transport::protocols::websocket::AllocatedResources {
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

/// Example 3: Receipt verification choreography
/// Shows anti-replay protection and consensus building
async fn receipt_verification_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Example 3: Receipt Verification Choreography\n");

    let coordinator_id = DeviceId::new();
    let verifier1_id = DeviceId::new();
    let verifier2_id = DeviceId::new();
    let context_id = ContextId::new();

    let choreo_config = ChoreographicConfig {
        max_concurrent_protocols: 15,
        protocol_timeout: std::time::Duration::from_secs(45),
        required_capabilities: vec![
            "receipt_verification".to_string(),
            "anti_replay_protection".to_string(),
        ],
        extension_registry: Default::default(),
    };

    let mut coordinator = ReceiptVerificationCoordinator::new(coordinator_id, choreo_config);

    println!("Receipt verification coordinator created");
    println!(
        "  Coordinator: {}",
        coordinator_id.to_hex()[..8].to_string()
    );
    println!("  Verifiers: 2 participants");

    // Create receipt data for verification
    let receipt_data = aura_transport::protocols::websocket::ReceiptData {
        receipt_id: "receipt-001".to_string(),
        sender_id: DeviceId::new(),
        recipient_id: DeviceId::new(),
        message_hash: vec![0x01, 0x02, 0x03, 0x04],
        signature: vec![0xAA, 0xBB, 0xCC, 0xDD],
        timestamp: SystemTime::now(),
        context_id,
    };

    // Initiate choreographic receipt verification
    let verification_id =
        coordinator.initiate_verification(receipt_data, vec![verifier1_id, verifier2_id])?;

    println!("\nMulti-party receipt verification initiated:");
    println!("  Verification ID: {}", &verification_id[..16]);

    // Simulate choreographic verification responses
    let response1 = aura_transport::protocols::websocket::ReceiptVerificationResponse {
        verification_id: verification_id.clone(),
        verifier_id: verifier1_id,
        verification_result: aura_transport::protocols::websocket::VerificationOutcome::Valid {
            confidence: 95,
        },
        verification_proof: vec![0x11, 0x22, 0x33],
        anti_replay_token: vec![0x44, 0x55, 0x66],
        timestamp: SystemTime::now(),
    };

    let response2 = aura_transport::protocols::websocket::ReceiptVerificationResponse {
        verification_id: verification_id.clone(),
        verifier_id: verifier2_id,
        verification_result: aura_transport::protocols::websocket::VerificationOutcome::Valid {
            confidence: 88,
        },
        verification_proof: vec![0x77, 0x88, 0x99],
        anti_replay_token: vec![0xAA, 0xBB, 0xCC],
        timestamp: SystemTime::now(),
    };

    // Process responses through choreographic coordinator
    let sufficient1 = coordinator.process_verification_response(response1)?;
    let sufficient2 = coordinator.process_verification_response(response2)?;

    // Build consensus through choreographic protocol
    let consensus = coordinator.build_consensus(&verification_id)?;

    println!("Choreographic verification completed:");
    println!("  Sufficient responses: {}", sufficient1 && sufficient2);
    println!("  Consensus result: {:?}", consensus);

    println!("\nMulti-party receipt verification with anti-replay protection");
    println!("Consensus building through choreographic coordination");
    println!("Session types ensure correct verification workflow\n");

    Ok(())
}

/// Example 4: Custom choreographic transport protocol
/// Shows how to define custom choreographic protocols using aura-macros
async fn custom_protocol_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎭 Example 4: Custom Choreographic Transport Protocol\n");

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
