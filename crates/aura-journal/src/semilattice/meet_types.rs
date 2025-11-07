//! Domain-specific meet semi-lattice CRDTs for Aura applications
//!
//! This module provides concrete implementations of meet semi-lattices
//! for common use cases: capability restriction, time window intersection,
//! security policy enforcement, and consensus constraints.

use aura_types::identifiers::DeviceId;
use aura_types::semilattice::{MeetSemiLattice, MvState, Top};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Capability set with meet-based restriction semantics
///
/// Represents access permissions that can be systematically restricted
/// through meet operations. Essential for distributed access control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Read access permissions (resource paths or patterns)
    pub read_permissions: BTreeSet<String>,
    /// Write access permissions  
    pub write_permissions: BTreeSet<String>,
    /// Administrative permissions
    pub admin_permissions: BTreeSet<String>,
    /// Capability expiry time (Unix timestamp)
    pub expiry_time: Option<u64>,
    /// Maximum operation count before capability expires
    pub max_operations: Option<u64>,
}

impl MeetSemiLattice for CapabilitySet {
    fn meet(&self, other: &Self) -> Self {
        Self {
            // Intersection of permissions (more restrictive)
            read_permissions: self
                .read_permissions
                .intersection(&other.read_permissions)
                .cloned()
                .collect(),
            write_permissions: self
                .write_permissions
                .intersection(&other.write_permissions)
                .cloned()
                .collect(),
            admin_permissions: self
                .admin_permissions
                .intersection(&other.admin_permissions)
                .cloned()
                .collect(),
            // Earlier expiry time (more restrictive)
            expiry_time: match (self.expiry_time, other.expiry_time) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            // Lower operation count (more restrictive)
            max_operations: match (self.max_operations, other.max_operations) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
        }
    }
}

impl Top for CapabilitySet {
    fn top() -> Self {
        // Most permissive state: universal permissions, no restrictions
        Self {
            read_permissions: ["*".to_string()].into_iter().collect(),
            write_permissions: ["*".to_string()].into_iter().collect(),
            admin_permissions: ["*".to_string()].into_iter().collect(),
            expiry_time: None,    // No expiry = perpetual access
            max_operations: None, // No operation limit
        }
    }
}

impl MvState for CapabilitySet {}

impl CapabilitySet {
    /// Create a read-only capability set for specific resources
    pub fn read_only(resources: Vec<String>) -> Self {
        Self {
            read_permissions: resources.into_iter().collect(),
            write_permissions: BTreeSet::new(),
            admin_permissions: BTreeSet::new(),
            expiry_time: None,
            max_operations: None,
        }
    }

    /// Create a capability set with time-based expiry
    pub fn with_expiry(mut self, expiry: u64) -> Self {
        self.expiry_time = Some(expiry);
        self
    }

    /// Create a capability set with operation count limit
    pub fn with_operation_limit(mut self, limit: u64) -> Self {
        self.max_operations = Some(limit);
        self
    }

    /// Check if capability set has expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.expiry_time
            .map_or(false, |expiry| current_time > expiry)
    }

    /// Check if capability allows specific resource access
    pub fn allows_read(&self, resource: &str) -> bool {
        self.read_permissions.contains("*") || self.read_permissions.contains(resource)
    }

    /// Check if capability allows write access
    pub fn allows_write(&self, resource: &str) -> bool {
        self.write_permissions.contains("*") || self.write_permissions.contains(resource)
    }
}

/// Time window with meet-based intersection for temporal constraints
///
/// Represents valid time ranges that can be intersected to find
/// overlapping periods. Critical for coordinating time-sensitive operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    /// Window start time (Unix timestamp)
    pub start: u64,
    /// Window end time (Unix timestamp)  
    pub end: u64,
    /// Time zone offset in seconds from UTC
    pub timezone_offset: Option<i32>,
}

impl MeetSemiLattice for TimeWindow {
    fn meet(&self, other: &Self) -> Self {
        Self {
            // Latest start time (more restrictive)
            start: self.start.max(other.start),
            // Earliest end time (more restrictive)
            end: self.end.min(other.end),
            // Use first timezone if different
            timezone_offset: self.timezone_offset.or(other.timezone_offset),
        }
    }
}

impl Top for TimeWindow {
    fn top() -> Self {
        // Universal time window covering all possible times
        Self {
            start: 0,
            end: u64::MAX,
            timezone_offset: Some(0), // UTC
        }
    }
}

impl MvState for TimeWindow {}

impl TimeWindow {
    /// Create a time window for a specific duration
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            start,
            end,
            timezone_offset: Some(0),
        }
    }

    /// Create a time window with timezone
    pub fn with_timezone(start: u64, end: u64, offset: i32) -> Self {
        Self {
            start,
            end,
            timezone_offset: Some(offset),
        }
    }

    /// Check if window is valid (start <= end)
    pub fn is_valid(&self) -> bool {
        self.start <= self.end
    }

    /// Check if a timestamp falls within the window
    pub fn contains(&self, timestamp: u64) -> bool {
        timestamp >= self.start && timestamp <= self.end
    }

    /// Get duration of the window in seconds
    pub fn duration(&self) -> u64 {
        if self.end >= self.start {
            self.end - self.start
        } else {
            0
        }
    }

    /// Check if this window overlaps with another
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// Security policy with meet-based intersection for policy composition
///
/// Represents security constraints that can be composed through intersection
/// to create increasingly restrictive combined policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityPolicy {
    /// Required authentication methods
    pub required_auth_methods: BTreeSet<String>,
    /// Minimum security level (higher is more secure)
    pub min_security_level: u8,
    /// Allowed network origins
    pub allowed_origins: BTreeSet<String>,
    /// Maximum session duration in seconds
    pub max_session_duration: Option<u64>,
    /// Required device capabilities
    pub required_device_caps: BTreeSet<String>,
}

impl MeetSemiLattice for SecurityPolicy {
    fn meet(&self, other: &Self) -> Self {
        Self {
            // Union of required auth methods (more restrictive)
            required_auth_methods: self
                .required_auth_methods
                .union(&other.required_auth_methods)
                .cloned()
                .collect(),
            // Higher security level requirement (more restrictive)
            min_security_level: self.min_security_level.max(other.min_security_level),
            // Intersection of allowed origins (more restrictive)
            allowed_origins: self
                .allowed_origins
                .intersection(&other.allowed_origins)
                .cloned()
                .collect(),
            // Shorter session duration (more restrictive)
            max_session_duration: match (self.max_session_duration, other.max_session_duration) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            // Union of required device capabilities (more restrictive)
            required_device_caps: self
                .required_device_caps
                .union(&other.required_device_caps)
                .cloned()
                .collect(),
        }
    }
}

impl Top for SecurityPolicy {
    fn top() -> Self {
        // Most permissive policy: no requirements or restrictions
        Self {
            required_auth_methods: BTreeSet::new(),
            min_security_level: 0,
            allowed_origins: ["*".to_string()].into_iter().collect(),
            max_session_duration: None,
            required_device_caps: BTreeSet::new(),
        }
    }
}

impl MvState for SecurityPolicy {}

/// Consensus constraint for threshold protocol coordination
///
/// Represents constraints on distributed consensus protocols that can be
/// intersected to find viable consensus parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusConstraint {
    /// Minimum number of participants required
    pub min_participants: u32,
    /// Maximum number of participants allowed
    pub max_participants: u32,
    /// Required threshold ratio (numerator, denominator)
    pub threshold_ratio: (u32, u32),
    /// Maximum consensus timeout in seconds
    pub max_timeout: u64,
    /// Required participant capabilities
    pub required_capabilities: BTreeSet<String>,
}

impl MeetSemiLattice for ConsensusConstraint {
    fn meet(&self, other: &Self) -> Self {
        Self {
            // Higher minimum participants (more restrictive)
            min_participants: self.min_participants.max(other.min_participants),
            // Lower maximum participants (more restrictive)
            max_participants: self.max_participants.min(other.max_participants),
            // Higher threshold ratio (more restrictive)
            threshold_ratio: {
                let self_ratio = self.threshold_ratio.0 as f64 / self.threshold_ratio.1 as f64;
                let other_ratio = other.threshold_ratio.0 as f64 / other.threshold_ratio.1 as f64;
                if self_ratio >= other_ratio {
                    self.threshold_ratio
                } else {
                    other.threshold_ratio
                }
            },
            // Shorter timeout (more restrictive)
            max_timeout: self.max_timeout.min(other.max_timeout),
            // Union of required capabilities (more restrictive)
            required_capabilities: self
                .required_capabilities
                .union(&other.required_capabilities)
                .cloned()
                .collect(),
        }
    }
}

impl Top for ConsensusConstraint {
    fn top() -> Self {
        // Most permissive consensus: minimal requirements
        Self {
            min_participants: 1,
            max_participants: u32::MAX,
            threshold_ratio: (1, 1), // 100% threshold = most permissive
            max_timeout: u64::MAX,
            required_capabilities: BTreeSet::new(),
        }
    }
}

impl MvState for ConsensusConstraint {}

impl ConsensusConstraint {
    /// Check if constraint parameters are valid
    pub fn is_valid(&self) -> bool {
        self.min_participants <= self.max_participants
            && self.threshold_ratio.0 <= self.threshold_ratio.1
            && self.threshold_ratio.1 > 0
    }

    /// Calculate required threshold count for given participant count
    pub fn required_threshold(&self, participant_count: u32) -> u32 {
        if participant_count == 0 {
            return 0;
        }

        let threshold_float = (participant_count as f64 * self.threshold_ratio.0 as f64)
            / self.threshold_ratio.1 as f64;
        threshold_float.ceil() as u32
    }
}

/// Resource quota constraint for resource allocation
///
/// Manages resource limits that can be intersected to ensure
/// resource allocation never exceeds the most restrictive constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceQuota {
    /// Maximum memory allocation in bytes
    pub max_memory: Option<u64>,
    /// Maximum storage usage in bytes
    pub max_storage: Option<u64>,
    /// Maximum network bandwidth in bytes/sec
    pub max_bandwidth: Option<u64>,
    /// Maximum CPU time in milliseconds
    pub max_cpu_time: Option<u64>,
    /// Maximum concurrent connections
    pub max_connections: Option<u32>,
}

impl MeetSemiLattice for ResourceQuota {
    fn meet(&self, other: &Self) -> Self {
        Self {
            max_memory: min_option(self.max_memory, other.max_memory),
            max_storage: min_option(self.max_storage, other.max_storage),
            max_bandwidth: min_option(self.max_bandwidth, other.max_bandwidth),
            max_cpu_time: min_option(self.max_cpu_time, other.max_cpu_time),
            max_connections: min_option(self.max_connections, other.max_connections),
        }
    }
}

impl Top for ResourceQuota {
    fn top() -> Self {
        // Unlimited resources
        Self {
            max_memory: None,
            max_storage: None,
            max_bandwidth: None,
            max_cpu_time: None,
            max_connections: None,
        }
    }
}

impl MvState for ResourceQuota {}

/// Helper function for computing minimum of optional values
fn min_option<T: Ord + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

/// Device capability constraint for device-specific restrictions
///
/// Represents capabilities that devices must support, with meet operations
/// ensuring compatibility across all participating devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCapability {
    /// Required cryptographic algorithms
    pub required_crypto_algs: BTreeSet<String>,
    /// Required protocol versions
    pub required_protocol_versions: BTreeSet<String>,
    /// Minimum hardware security level  
    pub min_hardware_security: u8,
    /// Required attestation mechanisms
    pub required_attestations: BTreeSet<String>,
    /// Supported device types
    pub supported_device_types: BTreeSet<String>,
}

impl MeetSemiLattice for DeviceCapability {
    fn meet(&self, other: &Self) -> Self {
        Self {
            // Union of required crypto algorithms (more restrictive)
            required_crypto_algs: self
                .required_crypto_algs
                .union(&other.required_crypto_algs)
                .cloned()
                .collect(),
            // Intersection of supported protocol versions (compatible subset)
            required_protocol_versions: self
                .required_protocol_versions
                .intersection(&other.required_protocol_versions)
                .cloned()
                .collect(),
            // Higher security level requirement (more restrictive)
            min_hardware_security: self.min_hardware_security.max(other.min_hardware_security),
            // Union of required attestations (more restrictive)
            required_attestations: self
                .required_attestations
                .union(&other.required_attestations)
                .cloned()
                .collect(),
            // Intersection of supported device types (compatibility)
            supported_device_types: self
                .supported_device_types
                .intersection(&other.supported_device_types)
                .cloned()
                .collect(),
        }
    }
}

impl Top for DeviceCapability {
    fn top() -> Self {
        // Universal device compatibility: no requirements
        Self {
            required_crypto_algs: BTreeSet::new(),
            required_protocol_versions: ["*".to_string()].into_iter().collect(),
            min_hardware_security: 0,
            required_attestations: BTreeSet::new(),
            supported_device_types: ["*".to_string()].into_iter().collect(),
        }
    }
}

impl MvState for DeviceCapability {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_set_meet() {
        let cap1 = CapabilitySet {
            read_permissions: ["file1", "file2"].iter().map(|s| s.to_string()).collect(),
            write_permissions: ["file1"].iter().map(|s| s.to_string()).collect(),
            admin_permissions: BTreeSet::new(),
            expiry_time: Some(1000),
            max_operations: Some(100),
        };

        let cap2 = CapabilitySet {
            read_permissions: ["file2", "file3"].iter().map(|s| s.to_string()).collect(),
            write_permissions: ["file1", "file2"].iter().map(|s| s.to_string()).collect(),
            admin_permissions: BTreeSet::new(),
            expiry_time: Some(2000),
            max_operations: Some(50),
        };

        let intersection = cap1.meet(&cap2);

        // Intersection should be more restrictive
        assert_eq!(intersection.read_permissions.len(), 1); // Only "file2"
        assert!(intersection.read_permissions.contains("file2"));
        assert_eq!(intersection.write_permissions.len(), 1); // Only "file1"
        assert!(intersection.write_permissions.contains("file1"));
        assert_eq!(intersection.expiry_time, Some(1000)); // Earlier expiry
        assert_eq!(intersection.max_operations, Some(50)); // Lower limit
    }

    #[test]
    fn test_time_window_intersection() {
        let window1 = TimeWindow::new(100, 500);
        let window2 = TimeWindow::new(200, 600);

        let intersection = window1.meet(&window2);

        assert_eq!(intersection.start, 200); // Later start
        assert_eq!(intersection.end, 500); // Earlier end
        assert!(intersection.is_valid());
        assert!(intersection.contains(300));
        assert!(!intersection.contains(150));
    }

    #[test]
    fn test_security_policy_intersection() {
        let policy1 = SecurityPolicy {
            required_auth_methods: ["password"].iter().map(|s| s.to_string()).collect(),
            min_security_level: 5,
            allowed_origins: ["*.example.com", "trusted.org"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            max_session_duration: Some(3600),
            required_device_caps: ["tpm"].iter().map(|s| s.to_string()).collect(),
        };

        let policy2 = SecurityPolicy {
            required_auth_methods: ["2fa"].iter().map(|s| s.to_string()).collect(),
            min_security_level: 3,
            allowed_origins: ["*.example.com"].iter().map(|s| s.to_string()).collect(),
            max_session_duration: Some(1800),
            required_device_caps: ["secure_boot"].iter().map(|s| s.to_string()).collect(),
        };

        let combined = policy1.meet(&policy2);

        // Should require both auth methods
        assert_eq!(combined.required_auth_methods.len(), 2);
        // Should use higher security level
        assert_eq!(combined.min_security_level, 5);
        // Should use shorter session duration
        assert_eq!(combined.max_session_duration, Some(1800));
        // Should require both device capabilities
        assert_eq!(combined.required_device_caps.len(), 2);
    }

    #[test]
    fn test_consensus_constraint_meet() {
        let constraint1 = ConsensusConstraint {
            min_participants: 3,
            max_participants: 10,
            threshold_ratio: (2, 3), // 67%
            max_timeout: 5000,
            required_capabilities: ["sign"].iter().map(|s| s.to_string()).collect(),
        };

        let constraint2 = ConsensusConstraint {
            min_participants: 5,
            max_participants: 8,
            threshold_ratio: (3, 4), // 75%
            max_timeout: 3000,
            required_capabilities: ["verify"].iter().map(|s| s.to_string()).collect(),
        };

        let intersection = constraint1.meet(&constraint2);

        assert_eq!(intersection.min_participants, 5); // Higher minimum
        assert_eq!(intersection.max_participants, 8); // Lower maximum
        assert_eq!(intersection.threshold_ratio, (3, 4)); // Higher threshold
        assert_eq!(intersection.max_timeout, 3000); // Shorter timeout
        assert_eq!(intersection.required_capabilities.len(), 2); // Both capabilities
        assert!(intersection.is_valid());
    }
}
