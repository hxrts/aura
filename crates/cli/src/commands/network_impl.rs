// Network and CGKA commands

use crate::commands::common;
use crate::config::Config;
use anyhow::Context;
use aura_agent::{Agent, StorageAgent};
use clap::Subcommand;
use std::collections::HashMap;
use tracing::{info, warn};

// Re-import extracted modules
use super::advanced_tests;
use super::basic_tests;
use super::capability_ops;
use super::e2e_tests;
use super::group_ops;
use super::helpers;
use super::peer_ops;
use super::status_ops;
use super::storage_tests;

#[derive(Subcommand)]
pub enum NetworkCommand {
    /// Connect to peer
    Connect {
        /// Peer individual ID
        peer_id: String,

        /// Peer address
        address: String,
    },

    /// Disconnect from peer
    Disconnect {
        /// Peer individual ID
        peer_id: String,
    },

    /// List connected peers
    Peers,

    /// Test P2P transport layer
    TestTransport {
        /// Transport type (stub, simple_tcp, noise_tcp, https_relay)
        #[arg(long, default_value = "simple_tcp")]
        transport_type: String,

        /// Listen address for transport
        #[arg(long, default_value = "127.0.0.1:9000")]
        listen_addr: String,

        /// Start as server (accept connections)
        #[arg(long)]
        server: bool,

        /// Connect to peer address (client mode)
        #[arg(long)]
        connect_to: Option<String>,
    },

    /// Create MLS group with network propagation
    CreateGroup {
        /// Group identifier
        group_id: String,

        /// Initial members (comma-separated individual IDs)
        #[arg(long)]
        members: String,
    },

    /// Send data to group members
    SendData {
        /// Target group ID
        group_id: String,

        /// Data to send
        data: String,

        /// Context for the data
        #[arg(long, default_value = "message")]
        context: String,
    },

    /// Delegate capability to peer via network
    DelegateCapability {
        /// Parent capability scope (namespace:operation)
        #[arg(long)]
        parent: String,

        /// Target subject ID
        #[arg(long)]
        subject: String,

        /// New capability scope (namespace:operation)
        #[arg(long)]
        scope: String,

        /// Optional resource constraint
        #[arg(long)]
        resource: Option<String>,

        /// Target peers (comma-separated, optional for broadcast)
        #[arg(long)]
        peers: Option<String>,

        /// Expiry timestamp (Unix seconds)
        #[arg(long)]
        expiry: Option<u64>,
    },

    /// Revoke capability via network
    RevokeCapability {
        /// Capability ID to revoke
        capability_id: String,

        /// Reason for revocation
        #[arg(long)]
        reason: String,

        /// Target peers (comma-separated, optional for broadcast)
        #[arg(long)]
        peers: Option<String>,
    },

    /// Show network statistics
    Stats,

    /// Show CGKA group status
    Groups,

    /// Process capability changes for group
    ProcessChanges {
        /// Group ID to process
        group_id: String,
    },

    /// Show pending network operations
    Pending,

    /// Test multiple agents running simultaneously on different ports
    MultiAgent {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Test duration in seconds
        #[arg(long, default_value = "10")]
        duration: u64,
    },

    /// Test peer discovery mechanism between agents
    PeerDiscovery {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Discovery test duration in seconds
        #[arg(long, default_value = "15")]
        duration: u64,
    },

    /// Test establishing connections between all device pairs
    EstablishConnections {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Connection test duration in seconds
        #[arg(long, default_value = "20")]
        duration: u64,
    },

    /// Test sending and receiving messages between all device pairs
    MessageExchange {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of messages to send per device pair
        #[arg(long, default_value = "5")]
        message_count: u32,

        /// Message exchange test duration in seconds
        #[arg(long, default_value = "30")]
        duration: u64,
    },

    /// Test network partition handling and reconnection
    PartitionTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Duration of network partition in seconds
        #[arg(long, default_value = "10")]
        partition_duration: u64,

        /// Total test duration in seconds
        #[arg(long, default_value = "45")]
        total_duration: u64,
    },

    /// Test storage operations with capability-based access control
    StorageTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of files to store per device
        #[arg(long, default_value = "5")]
        file_count: u32,

        /// File size in bytes
        #[arg(long, default_value = "1024")]
        file_size: u32,
    },

    /// Test data persistence across agent restarts
    PersistenceTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of files to store per device
        #[arg(long, default_value = "3")]
        file_count: u32,

        /// File size in bytes
        #[arg(long, default_value = "512")]
        file_size: u32,
    },

    /// Test storage replication between devices
    ReplicationTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of files to store per device
        #[arg(long, default_value = "2")]
        file_count: u32,

        /// File size in bytes
        #[arg(long, default_value = "1024")]
        file_size: u32,

        /// Replication factor (how many peers each file should be replicated to)
        #[arg(long, default_value = "2")]
        replication_factor: u16,
    },

    /// Test encrypted storage integrity and tamper detection
    EncryptionTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of files to store per device
        #[arg(long, default_value = "3")]
        file_count: u32,

        /// File size in bytes
        #[arg(long, default_value = "1024")]
        file_size: u32,

        /// Enable tamper detection testing
        #[arg(long)]
        test_tamper_detection: bool,
    },

    /// Test storage quota management and enforcement
    QuotaTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Storage quota limit per device in bytes
        #[arg(long, default_value = "8192")]
        quota_limit: u64,

        /// File size for testing quota limits
        #[arg(long, default_value = "2048")]
        file_size: u32,

        /// Test quota enforcement by exceeding limits
        #[arg(long)]
        test_quota_enforcement: bool,
    },

    /// Test capability revocation and access denial
    CapabilityTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of files to store for capability testing
        #[arg(long, default_value = "3")]
        file_count: u32,

        /// File size for capability testing
        #[arg(long, default_value = "1024")]
        file_size: u32,

        /// Test cross-device capability verification
        #[arg(long)]
        test_cross_device_access: bool,
    },

    /// Test protocol state machines: initiation, execution, completion
    ProtocolTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of protocol instances to run for testing
        #[arg(long, default_value = "3")]
        protocol_count: u32,

        /// Protocol types to test (comma-separated: dkd,recovery,resharing)
        #[arg(long, default_value = "dkd,recovery")]
        protocol_types: String,

        /// Test protocol cancellation and error handling
        #[arg(long)]
        test_error_scenarios: bool,

        /// Test concurrent protocol execution limits
        #[arg(long)]
        test_concurrency: bool,
    },

    /// Test ledger consistency: event generation, convergence, CRDT resolution
    LedgerTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Number of events to generate per device for testing
        #[arg(long, default_value = "10")]
        events_per_device: u32,

        /// Event types to test (comma-separated: dkd,epoch,device,guardian)
        #[arg(long, default_value = "dkd,epoch,device")]
        event_types: String,

        /// Test CRDT conflict resolution scenarios
        #[arg(long)]
        test_crdt_conflicts: bool,

        /// Test event ordering and causal consistency
        #[arg(long)]
        test_event_ordering: bool,

        /// Test ledger replay and state reconstruction
        #[arg(long)]
        test_replay: bool,

        /// Test compaction and garbage collection
        #[arg(long)]
        test_compaction: bool,

        /// Test merkle proof generation and validation
        #[arg(long)]
        test_merkle_proofs: bool,
    },

    /// Comprehensive end-to-end integration test combining all smoke test components
    E2EIntegrationTest {
        /// Number of devices to test (uses configs/device_1.toml, configs/device_2.toml, etc.)
        #[arg(long, default_value = "3")]
        device_count: u16,

        /// Base port number (devices will use port+index)
        #[arg(long, default_value = "58835")]
        base_port: u16,

        /// Test duration for network operations in seconds
        #[arg(long, default_value = "30")]
        test_duration: u64,

        /// Number of files to test for storage operations
        #[arg(long, default_value = "5")]
        file_count: u32,

        /// File size in bytes for storage testing
        #[arg(long, default_value = "1024")]
        file_size: u32,

        /// Number of events per device for ledger testing
        #[arg(long, default_value = "10")]
        events_per_device: u32,

        /// Enable comprehensive security testing
        #[arg(long)]
        test_security: bool,

        /// Enable performance metrics collection
        #[arg(long)]
        collect_metrics: bool,

        /// Generate detailed test report
        #[arg(long)]
        generate_report: bool,
    },
}

pub async fn handle_network_command(
    command: NetworkCommand,
    config: &Config,
) -> anyhow::Result<()> {
    match command {
        NetworkCommand::Connect { peer_id, address } => {
            peer_ops::connect_peer(config, &peer_id, &address).await
        }

        NetworkCommand::Disconnect { peer_id } => peer_ops::disconnect_peer(config, &peer_id).await,

        NetworkCommand::Peers => peer_ops::list_peers(config).await,

        NetworkCommand::CreateGroup { group_id, members } => {
            group_ops::create_group(config, &group_id, &members).await
        }

        NetworkCommand::SendData {
            group_id,
            data,
            context,
        } => group_ops::send_data(config, &group_id, &data, &context).await,

        NetworkCommand::DelegateCapability {
            parent,
            subject,
            scope,
            resource,
            peers,
            expiry,
        } => {
            capability_ops::delegate_capability(
                config,
                &parent,
                &subject,
                &scope,
                resource.as_deref(),
                peers.as_deref(),
                expiry,
            )
            .await
        }

        NetworkCommand::RevokeCapability {
            capability_id,
            reason,
            peers,
        } => capability_ops::revoke_capability(config, &capability_id, &reason, peers.as_deref()).await,

        NetworkCommand::Stats => status_ops::show_network_stats(config).await,

        NetworkCommand::Groups => status_ops::show_groups(config).await,

        NetworkCommand::ProcessChanges { group_id } => {
            status_ops::process_capability_changes(config, &group_id).await
        }

        NetworkCommand::Pending => status_ops::show_pending_operations(config).await,

        NetworkCommand::MultiAgent {
            device_count,
            base_port,
            duration,
        } => basic_tests::test_multi_agent(device_count, base_port, duration).await,

        NetworkCommand::PeerDiscovery {
            device_count,
            base_port,
            duration,
        } => basic_tests::test_peer_discovery(device_count, base_port, duration).await,

        NetworkCommand::EstablishConnections {
            device_count,
            base_port,
            duration,
        } => basic_tests::test_establish_connections(device_count, base_port, duration).await,

        NetworkCommand::MessageExchange {
            device_count,
            base_port,
            message_count,
            duration,
        } => basic_tests::test_message_exchange(device_count, base_port, message_count, duration).await,

        NetworkCommand::PartitionTest {
            device_count,
            base_port,
            partition_duration,
            total_duration,
        } => {
            basic_tests::test_network_partition(device_count, base_port, partition_duration, total_duration)
                .await
        }

        NetworkCommand::StorageTest {
            device_count,
            base_port,
            file_count,
            file_size,
        } => storage_tests::test_storage_operations(device_count, base_port, file_count, file_size).await,

        NetworkCommand::PersistenceTest {
            device_count,
            base_port,
            file_count,
            file_size,
        } => storage_tests::test_storage_persistence(device_count, base_port, file_count, file_size).await,

        NetworkCommand::ReplicationTest {
            device_count,
            base_port,
            file_count,
            file_size,
            replication_factor,
        } => {
            storage_tests::test_storage_replication(
                device_count,
                base_port,
                file_count,
                file_size,
                replication_factor,
            )
            .await
        }

        NetworkCommand::EncryptionTest {
            device_count,
            base_port,
            file_count,
            file_size,
            test_tamper_detection,
        } => {
            storage_tests::test_encryption_integrity(
                device_count,
                base_port,
                file_count,
                file_size,
                test_tamper_detection,
            )
            .await
        }

        NetworkCommand::QuotaTest {
            device_count,
            base_port,
            quota_limit,
            file_size,
            test_quota_enforcement,
        } => {
            storage_tests::test_storage_quota_management(
                device_count,
                base_port,
                quota_limit,
                file_size,
                test_quota_enforcement,
            )
            .await
        }

        NetworkCommand::CapabilityTest {
            device_count,
            base_port,
            file_count,
            file_size,
            test_cross_device_access,
        } => {
            advanced_tests::test_capability_revocation_and_access_denial(
                device_count,
                base_port,
                file_count,
                file_size,
                test_cross_device_access,
            )
            .await
        }

        NetworkCommand::ProtocolTest {
            device_count,
            base_port,
            protocol_count,
            protocol_types,
            test_error_scenarios,
            test_concurrency,
        } => {
            advanced_tests::test_protocol_state_machines(
                device_count,
                base_port,
                protocol_count,
                &protocol_types,
                test_error_scenarios,
                test_concurrency,
            )
            .await
        }

        NetworkCommand::LedgerTest {
            device_count,
            base_port,
            events_per_device,
            event_types,
            test_crdt_conflicts,
            test_event_ordering,
            test_replay,
            test_compaction,
            test_merkle_proofs,
        } => {
            advanced_tests::test_ledger_consistency(
                device_count,
                base_port,
                events_per_device,
                &event_types,
                test_crdt_conflicts,
                test_event_ordering,
                test_replay,
                test_compaction,
                test_merkle_proofs,
            )
            .await
        }

        NetworkCommand::E2EIntegrationTest {
            device_count,
            base_port,
            test_duration,
            file_count,
            file_size,
            events_per_device,
            test_security,
            collect_metrics,
            generate_report,
        } => {
            e2e_tests::test_e2e_integration(
                device_count,
                base_port,
                test_duration,
                file_count,
                file_size,
                events_per_device,
                test_security,
                collect_metrics,
                generate_report,
            )
            .await
        }
    }
}

// Peer operations moved to peer_ops module
