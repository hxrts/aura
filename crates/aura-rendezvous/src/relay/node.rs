//! Relay Node Types
//!
//! Types and structures for individual relay nodes.

use aura_core::{DeviceId, TrustLevel};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

/// Individual relay node with capabilities
#[derive(Debug, Clone)]
pub struct RelayNode {
    /// Node identifier
    pub node_id: DeviceId,
    /// Relay capabilities offered
    pub capabilities: RelayCapabilities,
    /// Node status
    pub status: RelayStatus,
    /// Geographic/network location info
    pub location_info: LocationInfo,
    /// Performance characteristics
    pub performance: RelayPerformance,
    /// Trust and reputation
    pub trust_info: RelayTrustInfo,
}

/// Capabilities offered by relay node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCapabilities {
    /// Maximum concurrent connections
    pub max_connections: u32,
    /// Supported message types
    pub supported_message_types: HashSet<String>,
    /// Bandwidth limitations
    pub bandwidth_limits: BandwidthLimits,
    /// Storage capabilities for offline messages
    pub storage_capabilities: StorageCapabilities,
    /// Privacy features supported
    pub privacy_features: PrivacyFeatures,
    /// Quality of service guarantees
    pub qos_guarantees: QosGuarantees,
}

/// Relay node status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelayStatus {
    /// Online and accepting connections
    Active,
    /// Online but at capacity
    Busy,
    /// Temporarily offline
    Offline,
    /// Maintenance mode
    Maintenance,
    /// Permanently unavailable
    Disabled,
}

/// Geographic and network location information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationInfo {
    /// Approximate geographic region
    pub region: String,
    /// Network latency characteristics
    pub latency_profile: LatencyProfile,
    /// Network provider information
    pub network_provider: Option<String>,
    /// Connectivity type
    pub connectivity_type: ConnectivityType,
}

/// Relay performance characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPerformance {
    /// Average latency in milliseconds
    pub average_latency_ms: u32,
    /// Message throughput per second
    pub throughput_messages_per_sec: f64,
    /// Uptime percentage
    pub uptime_percentage: f32,
    /// Error rate
    pub error_rate: f32,
    /// Load factor (0.0 to 1.0)
    pub current_load: f32,
}

/// Trust and reputation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayTrustInfo {
    /// Overall trust level
    pub trust_level: TrustLevel,
    /// Reputation score
    pub reputation_score: f32,
    /// Number of successful relays
    pub successful_relays: u64,
    /// Number of failed relays
    pub failed_relays: u64,
    /// Attestations from other nodes
    pub attestations: Vec<TrustAttestation>,
}

/// Bandwidth limitations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthLimits {
    /// Maximum inbound bandwidth in bytes/sec
    pub max_inbound_bps: u64,
    /// Maximum outbound bandwidth in bytes/sec
    pub max_outbound_bps: u64,
    /// Message size limits
    pub max_message_size: u32,
    /// Rate limiting per connection
    pub rate_limit_per_connection: u32,
}

/// Storage capabilities for offline messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCapabilities {
    /// Maximum storage size in bytes
    pub max_storage_bytes: u64,
    /// Message retention time in seconds
    pub retention_time_seconds: u64,
    /// Support for encrypted storage
    pub encrypted_storage: bool,
    /// Forward secrecy for stored messages
    pub forward_secrecy: bool,
}

/// Privacy features supported by relay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyFeatures {
    /// Support for message mixing
    pub message_mixing: bool,
    /// Support for timing obfuscation
    pub timing_obfuscation: bool,
    /// Support for size padding
    pub size_padding: bool,
    /// Support for cover traffic
    pub cover_traffic: bool,
    /// Onion routing support
    pub onion_routing: bool,
}

/// Quality of service guarantees
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QosGuarantees {
    /// Maximum delivery delay in seconds
    pub max_delivery_delay_sec: u32,
    /// Delivery success rate guarantee
    pub delivery_success_rate: f32,
    /// Priority message support
    pub priority_support: bool,
    /// Guaranteed bandwidth per connection
    pub guaranteed_bandwidth_bps: Option<u64>,
}

/// Network latency characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyProfile {
    /// Minimum observed latency
    pub min_latency_ms: u32,
    /// Maximum observed latency
    pub max_latency_ms: u32,
    /// Average latency
    pub avg_latency_ms: u32,
    /// Latency jitter
    pub jitter_ms: u32,
}

/// Network connectivity types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectivityType {
    /// High-speed fiber connection
    Fiber,
    /// Cable/DSL connection
    Broadband,
    /// Mobile/cellular connection
    Mobile,
    /// Satellite connection
    Satellite,
    /// Unknown connection type
    Unknown,
}

/// Trust attestation from another node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustAttestation {
    /// Attesting node
    pub attesting_node: DeviceId,
    /// Trust score given
    pub trust_score: f32,
    /// Attestation timestamp
    pub timestamp: u64,
    /// Attestation signature
    pub signature: Vec<u8>,
}

/// Relay stream message for SBB forwarding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStream {
    /// Stream identifier
    pub stream_id: [u8; 32],
    /// Source device (encrypted, relay cannot see)
    pub encrypted_source: Vec<u8>,
    /// Destination device (encrypted, relay cannot see)
    pub encrypted_destination: Vec<u8>,
    /// Stream lifecycle state
    pub stream_state: StreamState,
    /// Encrypted payload (end-to-end encrypted)
    pub encrypted_payload: Vec<u8>,
    /// Flow control sequence number
    pub sequence_number: u64,
    /// Stream flags for control
    pub flags: StreamFlags,
}

/// Stream lifecycle states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamState {
    /// Initialize new stream
    Open,
    /// Stream data packet
    Data,
    /// Close stream gracefully
    Close,
    /// Force close stream (error)
    Reset,
}

/// Stream control flags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamFlags {
    /// High priority stream
    pub priority: bool,
    /// Requires acknowledgment
    pub ack_required: bool,
    /// End of stream marker
    pub fin: bool,
    /// Request flow budget check
    pub flow_check: bool,
}

impl RelayStream {
    /// Create new stream open message
    pub fn new_open(
        stream_id: [u8; 32],
        encrypted_source: Vec<u8>,
        encrypted_destination: Vec<u8>,
    ) -> Self {
        Self {
            stream_id,
            encrypted_source,
            encrypted_destination,
            stream_state: StreamState::Open,
            encrypted_payload: Vec::new(),
            sequence_number: 0,
            flags: StreamFlags {
                priority: false,
                ack_required: false,
                fin: false,
                flow_check: false,
            },
        }
    }

    /// Create new data stream message
    pub fn new_data(stream_id: [u8; 32], encrypted_payload: Vec<u8>, sequence_number: u64) -> Self {
        Self {
            stream_id,
            encrypted_source: Vec::new(),
            encrypted_destination: Vec::new(),
            stream_state: StreamState::Data,
            encrypted_payload,
            sequence_number,
            flags: StreamFlags {
                priority: false,
                ack_required: false,
                fin: false,
                flow_check: true,
            },
        }
    }

    /// Create stream close message
    pub fn new_close(stream_id: [u8; 32]) -> Self {
        Self {
            stream_id,
            encrypted_source: Vec::new(),
            encrypted_destination: Vec::new(),
            stream_state: StreamState::Close,
            encrypted_payload: Vec::new(),
            sequence_number: 0,
            flags: StreamFlags {
                priority: false,
                ack_required: true,
                fin: true,
                flow_check: false,
            },
        }
    }

    /// Check if stream preserves end-to-end encryption
    pub fn is_end_to_end_encrypted(&self) -> bool {
        !self.encrypted_payload.is_empty() || !self.encrypted_source.is_empty()
    }

    /// Get stream size for flow control
    pub fn stream_size(&self) -> usize {
        self.encrypted_payload.len()
            + self.encrypted_source.len()
            + self.encrypted_destination.len()
            + 64 // overhead
    }
}

/// Relay performance metrics
#[derive(Debug, Clone)]
pub struct RelayMetrics {
    /// Node identifier
    pub node_id: DeviceId,
    /// Recent performance samples
    pub performance_samples: VecDeque<PerformanceSample>,
    /// Aggregate statistics
    pub aggregate_stats: AggregateStats,
    /// Last metrics update
    pub last_updated: u64,
}

/// Individual performance sample
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSample {
    /// Sample timestamp
    pub timestamp: u64,
    /// Latency in milliseconds
    pub latency_ms: u32,
    /// Success/failure indicator
    pub success: bool,
    /// Message size
    pub message_size: u32,
    /// Processing time
    pub processing_time_ms: u32,
}

/// Aggregate performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateStats {
    /// Total messages relayed
    pub total_messages: u64,
    /// Success rate
    pub success_rate: f32,
    /// Average latency
    pub avg_latency_ms: f32,
    /// 95th percentile latency
    pub p95_latency_ms: f32,
    /// Throughput messages per second
    pub throughput_msg_per_sec: f32,
}
