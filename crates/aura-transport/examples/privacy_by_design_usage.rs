//! Privacy-by-Design Transport Usage Examples
//!
//! This example demonstrates how privacy preservation is integrated
//! directly into the core transport types, not bolted on as an afterthought.
//!
//! Key principles:
//! - Privacy mechanisms are built into core types
//! - Default behavior preserves privacy
//! - Context scoping is fundamental to all operations

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_transport::{
    ConnectionId, Envelope, HolePunchMessage, PeerInfo, PrivacyAwareSelectionCriteria,
    PrivacyLevel, StunMessage, TransportConfig,
};
use std::time::Duration;

fn main() {
    println!("=== Privacy-by-Design Transport Usage Examples ===\n");

    // Example 1: Privacy-aware envelope creation
    privacy_aware_envelope_example();

    // Example 2: Transport configuration with privacy levels
    transport_configuration_example();

    // Example 3: Privacy-preserving peer selection
    peer_selection_example();

    // Example 4: Context-scoped connections
    context_scoped_connections_example();

    // Example 5: Protocol messages with built-in privacy
    protocol_privacy_example();
}

/// Example 1: Privacy-aware envelope creation
/// Shows how privacy is integrated into the core Envelope type
fn privacy_aware_envelope_example() {
    println!("📧 Example 1: Privacy-Aware Envelope Creation\n");

    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let message = b"Hello, private world!".to_vec();

    // Create basic envelope - privacy level determined by configuration
    let basic_envelope = Envelope::new(message.clone());

    println!("Basic envelope created with default privacy settings");
    println!("Message size: {} bytes", basic_envelope.payload.len());
    println!("Privacy level: {:?}", basic_envelope.privacy_level());

    // Create context-scoped envelope - privacy built-in
    let scoped_envelope = Envelope::new_scoped(
        message.clone(),
        context_id,
        Some("secure_messaging".to_string()),
    );

    println!("\nScoped envelope created with context privacy");
    println!("Privacy level: {:?}", scoped_envelope.privacy_level());
    println!(
        "Requires context scope: {}",
        scoped_envelope.requires_context_scope()
    );

    // Create blinded envelope - maximum privacy
    let blinded_envelope = Envelope::new_blinded(message);

    println!("\nBlinded envelope created with maximum privacy");
    println!("Privacy level: {:?}", blinded_envelope.privacy_level());
    println!("Privacy preservation is automatic, not optional\n");
}

/// Example 2: Transport configuration with privacy levels
/// Shows how privacy levels are integrated into transport configuration
fn transport_configuration_example() {
    println!("⚙️  Example 2: Transport Configuration with Privacy Levels\n");

    // Maximum privacy configuration - context scoped
    let max_privacy_config = TransportConfig {
        privacy_level: PrivacyLevel::ContextScoped,
        max_connections: 50,
        connection_timeout: Duration::from_secs(30),
        enable_capability_filtering: true,
        default_blinding: true,
        ..Default::default()
    };

    println!("Max Privacy Config:");
    println!("  Privacy Level: {:?}", max_privacy_config.privacy_level);
    println!(
        "  Capability Filtering: {}",
        max_privacy_config.enable_capability_filtering
    );
    println!(
        "  Default Blinding: {}",
        max_privacy_config.default_blinding
    );

    // Balanced privacy configuration - selective blinding
    let balanced_config = TransportConfig {
        privacy_level: PrivacyLevel::Blinded,
        max_connections: 100,
        connection_timeout: Duration::from_secs(20),
        enable_capability_filtering: true,
        default_blinding: false,
        ..Default::default()
    };

    println!("\nBalanced Config:");
    println!("  Privacy Level: {:?}", balanced_config.privacy_level);
    println!("  Performance optimized while preserving core privacy");

    // Clear configuration - for testing or debugging only
    let clear_config = TransportConfig {
        privacy_level: PrivacyLevel::Clear,
        max_connections: 200,
        connection_timeout: Duration::from_secs(10),
        enable_capability_filtering: false,
        default_blinding: false,
        ..Default::default()
    };

    println!("\nClear Config (Testing Only):");
    println!("  Privacy Level: {:?}", clear_config.privacy_level);
    println!("  ⚠️  Only for testing - no privacy protection");
    println!("Privacy levels are explicit and built into configuration\n");
}

/// Example 3: Privacy-preserving peer selection
/// Shows how peer discovery and selection preserve privacy by default
fn peer_selection_example() {
    println!("👥 Example 3: Privacy-Preserving Peer Selection\n");

    // Create some sample peers with privacy-aware information
    let mut peers = Vec::new();

    for i in 0..5 {
        let authority_id = AuthorityId::new_from_entropy([i as u8; 32]);
        let mut peer_info = PeerInfo::new(authority_id);

        // Add context for the peer
        let context_id = ContextId::new_from_entropy([i as u8; 32]);
        peer_info.add_context(context_id);

        peers.push(peer_info);
    }

    println!(
        "Created {} peers with privacy-aware information",
        peers.len()
    );

    // Privacy-aware selection criteria
    let mut selection_criteria = PrivacyAwareSelectionCriteria::new();
    selection_criteria.require_capability("secure_messaging".to_string());
    selection_criteria.min_reliability(aura_transport::peers::info::ReliabilityLevel::Medium);

    println!("Selection criteria preserves privacy:");
    println!("  Selection is privacy-preserving by design");

    // Select peers using privacy-aware criteria
    let peer_vec = peers.to_vec();
    let selection_result = selection_criteria.select_peers(peer_vec);

    println!(
        "\nSelected {} peers meeting privacy-aware criteria",
        selection_result.selected_peers.len()
    );
    println!(
        "Average selection score: {:.2}",
        selection_result.average_score()
    );
    println!("Peer selection preserves capability privacy\n");
}

/// Example 4: Context-scoped connections
/// Shows how connections are scoped to specific contexts
fn context_scoped_connections_example() {
    println!("🔗 Example 4: Context-Scoped Connections\n");

    let _authority_id = AuthorityId::new_from_entropy([0u8; 32]);
    let peer_authority = AuthorityId::new_from_entropy([1u8; 32]);
    let family_context = ContextId::new_from_entropy([1u8; 32]);
    let work_context = ContextId::new_from_entropy([2u8; 32]);

    // Create basic connection IDs
    let basic_connection = ConnectionId::new();

    // Create context-scoped connection IDs
    let family_connection =
        aura_transport::ScopedConnectionId::new(basic_connection, family_context);
    let work_connection = aura_transport::ScopedConnectionId::new(basic_connection, work_context);

    println!("Created context-scoped connections:");
    println!(
        "  Family connection ID: {:?}",
        family_connection.connection_id()
    );
    println!(
        "  Work connection ID: {:?}",
        work_connection.connection_id()
    );

    // Connections are isolated by context
    assert_ne!(family_connection.context_id(), work_connection.context_id());
    println!("\nSame peer, different contexts = different connection scopes");
    println!("Context scoping prevents cross-context leakage");

    // Create connection info with privacy
    let connection_info = aura_transport::ConnectionInfo::new_scoped(
        peer_authority,
        family_context,
        PrivacyLevel::ContextScoped,
    );

    println!("\nConnection metadata:");
    println!(
        "  Connection established: {}",
        connection_info.is_established()
    );
    println!("  Age: {:?}", connection_info.age());
    println!("Privacy preservation is built into connection metadata\n");
}

/// Example 5: Protocol messages with built-in privacy
/// Shows how protocol messages integrate privacy mechanisms
fn protocol_privacy_example() {
    println!("📡 Example 5: Protocol Messages with Built-in Privacy\n");

    // STUN message with privacy-aware construction
    let _stun_message = StunMessage::binding_request();

    println!("STUN message created:");
    println!("  Binding request message type");
    println!("  Transaction ID included for privacy");

    // Hole punch message with basic construction
    let target_addr = aura_transport::types::endpoint::EndpointAddress::new("192.168.1.100:12345");
    let _hole_punch_msg = HolePunchMessage::coordination_request(
        AuthorityId::new_from_entropy([1u8; 32]),
        AuthorityId::new_from_entropy([2u8; 32]),
        target_addr,
    );

    println!("\nHole punch message created:");
    println!("  Target endpoint configured");
    println!("  Built-in timing and retry logic");

    println!("\nProtocol messages have privacy built-in by default");
    println!("Privacy is not optional - it's fundamental to the design");
}
