//! Capability verification messages
//!
//! Messages for announcing and verifying device and protocol capabilities.

use crate::serialization::WireSerializable;
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};

/// Capability message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityMessage {
    /// Announce device capabilities
    Announce(CapabilityAnnouncement),
    /// Request capability verification
    VerificationRequest(CapabilityVerificationRequest),
    /// Capability verification response
    VerificationResponse(CapabilityVerificationResponse),
    /// Capability challenge
    Challenge(CapabilityChallenge),
    /// Challenge response
    ChallengeResponse(CapabilityChallengeResponse),
}

/// Device capability announcement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityAnnouncement {
    pub device_id: DeviceId,
    pub protocol_capabilities: Vec<ProtocolCapability>,
    pub transport_capabilities: Vec<TransportCapability>,
    pub storage_capabilities: Option<StorageCapability>,
    pub computational_capabilities: ComputationalCapability,
    pub announcement_timestamp: u64,
    pub capability_signature: Vec<u8>,
}

/// Protocol-specific capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolCapability {
    pub protocol_type: String,
    pub version: String,
    pub role: ProtocolRole,
    pub features: Vec<String>,
    pub performance_metrics: Option<PerformanceMetrics>,
}

/// Role in protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolRole {
    Participant,
    Coordinator,
    Observer,
    Validator,
}

/// Performance metrics for capability assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub latency_ms: Option<u64>,
    pub throughput_ops_per_sec: Option<u64>,
    pub reliability_score: Option<f64>, // 0.0 to 1.0
    pub uptime_percentage: Option<f64>,
}

/// Transport-specific capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportCapability {
    pub transport_type: String,
    pub supported_features: Vec<String>,
    pub max_bandwidth_mbps: Option<u64>,
    pub encryption_support: Vec<String>,
    pub nat_traversal: bool,
}

/// Storage capability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCapability {
    pub available_capacity_bytes: u64,
    pub max_chunk_size_bytes: u64,
    pub encryption_at_rest: bool,
    pub replication_factor: u8,
    pub accepting_new_data: bool,
    pub storage_class: StorageClass,
}

/// Storage class for different types of data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageClass {
    HotStorage,      // Frequently accessed
    WarmStorage,     // Occasionally accessed
    ColdStorage,     // Rarely accessed
    ArchivalStorage, // Long-term retention
}

/// Computational capability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputationalCapability {
    pub cpu_cores: Option<u32>,
    pub memory_gb: Option<u32>,
    pub crypto_acceleration: bool,
    pub specialized_hardware: Vec<String>,
    pub max_concurrent_protocols: u32,
}

/// Capability verification request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityVerificationRequest {
    pub requesting_device: DeviceId,
    pub target_device: DeviceId,
    pub verification_type: VerificationType,
    pub challenge_data: Option<Vec<u8>>,
    pub timeout_seconds: u64,
}

/// Types of capability verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationType {
    ProtocolExecution { protocol_type: String },
    CryptographicOperation { operation_type: String },
    StorageOperation { operation_type: String },
    PerformanceBenchmark { benchmark_type: String },
}

/// Capability verification response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityVerificationResponse {
    pub responding_device: DeviceId,
    pub verification_successful: bool,
    pub response_data: Option<Vec<u8>>,
    pub performance_data: Option<PerformanceMetrics>,
    pub error_message: Option<String>,
    pub verification_signature: Vec<u8>,
}

/// Capability challenge for proof of capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityChallenge {
    pub challenger_device: DeviceId,
    pub challenged_device: DeviceId,
    pub challenge_type: ChallengeType,
    pub challenge_data: Vec<u8>,
    pub expected_response_format: String,
    pub deadline: u64,
}

/// Types of capability challenges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChallengeType {
    CryptographicProof,
    ProtocolExecution,
    StorageTest,
    ComputationTest,
    NetworkLatency,
}

/// Challenge response with proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityChallengeResponse {
    pub responding_device: DeviceId,
    pub challenge_response: Vec<u8>,
    pub proof_of_work: Option<Vec<u8>>,
    pub completion_time_ms: u64,
    pub response_signature: Vec<u8>,
}

// Implement wire serialization for all capability message types
impl WireSerializable for CapabilityMessage {}
impl WireSerializable for CapabilityAnnouncement {}
impl WireSerializable for ProtocolCapability {}
impl WireSerializable for PerformanceMetrics {}
impl WireSerializable for TransportCapability {}
impl WireSerializable for StorageCapability {}
impl WireSerializable for ComputationalCapability {}
impl WireSerializable for CapabilityVerificationRequest {}
impl WireSerializable for CapabilityVerificationResponse {}
impl WireSerializable for CapabilityChallenge {}
impl WireSerializable for CapabilityChallengeResponse {}
