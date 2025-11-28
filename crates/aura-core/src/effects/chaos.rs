//! Chaos Effects
//!
//! Provides controlled fault injection capabilities for chaos engineering and
//! resilience testing. These effects enable systematic testing of system behavior
//! under adverse conditions including network failures, Byzantine faults, and timing issues.
//!
//! # Effect Classification
//!
//! - **Category**: Testing/Simulation Effect
//! - **Implementation**: `aura-simulator` (Layer 8)
//! - **Usage**: Fault injection for chaos engineering, resilience testing
//!
//! This is a testing/simulation effect for controlled fault injection. Enables
//! systematic testing of Byzantine behavior, network partitions, corruption, and
//! resource constraints. Handlers in `aura-simulator` inject faults into the
//! deterministic simulation environment.

use crate::AuraError;
use async_trait::async_trait;
use std::time::Duration;

/// Chaos engineering operations for fault injection and resilience testing
///
/// This trait provides pure chaos primitives that can be composed to create
/// comprehensive fault injection scenarios. All operations are stateless and
/// work through explicit dependency injection for deterministic testing.
#[async_trait]
pub trait ChaosEffects {
    /// Inject message corruption faults
    ///
    /// Randomly corrupts network messages to test input validation
    /// and error handling in distributed protocols.
    ///
    /// # Arguments
    /// * `corruption_rate` - Probability of corrupting each message (0.0 to 1.0)
    /// * `corruption_type` - Type of corruption to inject
    ///
    /// # Returns
    /// Success indication or error if injection setup failed
    async fn inject_message_corruption(
        &self,
        corruption_rate: f64,
        corruption_type: CorruptionType,
    ) -> Result<(), ChaosError>;

    /// Inject network delay faults
    ///
    /// Adds artificial delays to network communication to test
    /// timeout handling and create timing-dependent race conditions.
    ///
    /// # Arguments
    /// * `delay_range` - Range of delays to inject (min, max)
    /// * `affected_peers` - Specific peers to target, or None for all
    ///
    /// # Returns
    /// Success indication or error if injection setup failed
    async fn inject_network_delay(
        &self,
        delay_range: (Duration, Duration),
        affected_peers: Option<Vec<String>>,
    ) -> Result<(), ChaosError>;

    /// Inject network partition faults
    ///
    /// Creates network partitions where subsets of participants
    /// cannot communicate with each other.
    ///
    /// # Arguments
    /// * `partition_groups` - Groups of peers that can communicate within group
    /// * `duration` - How long to maintain the partition
    ///
    /// # Returns
    /// Success indication or error if partition setup failed
    async fn inject_network_partition(
        &self,
        partition_groups: Vec<Vec<String>>,
        duration: Duration,
    ) -> Result<(), ChaosError>;

    /// Inject Byzantine behavior faults
    ///
    /// Makes participants send conflicting or malicious messages
    /// to test Byzantine fault tolerance.
    ///
    /// # Arguments
    /// * `byzantine_peers` - Peers to make Byzantine
    /// * `behavior_type` - Type of Byzantine behavior to exhibit
    ///
    /// # Returns
    /// Success indication or error if Byzantine setup failed
    async fn inject_byzantine_behavior(
        &self,
        byzantine_peers: Vec<String>,
        behavior_type: ByzantineType,
    ) -> Result<(), ChaosError>;

    /// Inject resource exhaustion faults
    ///
    /// Simulates resource constraints like memory limits,
    /// CPU exhaustion, or storage failures.
    ///
    /// # Arguments
    /// * `resource_type` - Type of resource to constrain
    /// * `constraint_level` - Severity of the constraint (0.0 to 1.0)
    ///
    /// # Returns
    /// Success indication or error if constraint setup failed
    async fn inject_resource_exhaustion(
        &self,
        resource_type: ResourceType,
        constraint_level: f64,
    ) -> Result<(), ChaosError>;

    /// Inject timing faults
    ///
    /// Manipulates timing to create race conditions and test
    /// temporal assumptions in protocols.
    ///
    /// # Arguments
    /// * `time_skew` - Amount to skew logical time
    /// * `clock_drift_rate` - Rate of clock drift to inject
    ///
    /// # Returns
    /// Success indication or error if timing injection setup failed
    async fn inject_timing_faults(
        &self,
        time_skew: Duration,
        clock_drift_rate: f64,
    ) -> Result<(), ChaosError>;

    /// Stop all active fault injections
    ///
    /// Disables all previously configured fault injection
    /// to return system to normal operation.
    ///
    /// # Returns
    /// Success indication or error if cleanup failed
    async fn stop_all_injections(&self) -> Result<(), ChaosError>;
}

/// Types of message corruption that can be injected
#[derive(Debug, Clone)]
pub enum CorruptionType {
    /// Flip random bits in the message
    BitFlip,
    /// Truncate messages randomly
    Truncation,
    /// Duplicate parts of the message
    Duplication,
    /// Insert random bytes
    Insertion,
    /// Swap byte order
    Reordering,
}

/// Types of Byzantine behavior to inject
#[derive(Debug, Clone)]
pub enum ByzantineType {
    /// Send conflicting messages to different peers
    Equivocation,
    /// Send messages with invalid signatures
    InvalidSignature,
    /// Send messages out of protocol order
    ProtocolViolation,
    /// Remain silent and stop sending messages
    Silent,
    /// Send random noise instead of valid messages
    Random,
}

/// Types of resources that can be constrained
#[derive(Debug, Clone)]
pub enum ResourceType {
    /// Memory/RAM constraints
    Memory,
    /// CPU processing constraints
    Cpu,
    /// Network bandwidth constraints
    NetworkBandwidth,
    /// Storage space constraints
    Storage,
    /// File descriptor limits
    FileDescriptors,
}

/// Errors that can occur during chaos injection operations
#[derive(Debug, thiserror::Error)]
pub enum ChaosError {
    /// Invalid chaos configuration
    #[error("Invalid chaos configuration: {reason}")]
    InvalidConfiguration { reason: String },

    /// Failed to inject specific fault type
    #[error("Failed to inject {fault_type} fault: {reason}")]
    InjectionFailed { fault_type: String, reason: String },

    /// Chaos injection not supported in current environment
    #[error("Chaos injection '{operation}' not supported: {reason}")]
    NotSupported { operation: String, reason: String },

    /// System error during chaos operations
    #[error("Chaos system error: {0}")]
    SystemError(#[from] AuraError),
}
