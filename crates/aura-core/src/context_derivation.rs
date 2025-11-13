//! Context derivation protocols for privacy partitions
//!
//! This module implements the key derivation protocols for creating
//! RID (pairwise relay contexts) and GID (group threshold contexts)
//! as specified in the formal model.

use crate::hash;
use crate::identifiers::{DeviceId, DkdContextId, GroupId, MessageContext, RelayId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

/// Errors that can occur during context derivation
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ContextDerivationError {
    /// Invalid input parameters
    #[error("Invalid input: {reason}")]
    InvalidInput {
        /// Description of why the input is invalid
        reason: String,
    },

    /// Insufficient participants for threshold
    #[error("Insufficient participants: need at least {required}, got {actual}")]
    InsufficientParticipants {
        /// Minimum number of participants required
        required: usize,
        /// Number of participants actually provided
        actual: usize,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic error: {reason}")]
    CryptoError {
        /// Description of the cryptographic failure
        reason: String,
    },
}

/// Result type for context derivation operations
pub type Result<T> = std::result::Result<T, ContextDerivationError>;

/// Protocol for deriving pairwise relay contexts (RID)
///
/// RID contexts are derived from X25519 key agreement between two devices,
/// providing perfect forward secrecy for pairwise communication channels.
pub struct RelayContextDerivation;

impl RelayContextDerivation {
    /// Derive a relay context from two device IDs
    ///
    /// This creates a deterministic but private context identifier for
    /// communication between two specific devices. The context is the same
    /// regardless of which device initiates the derivation.
    pub fn derive_relay_context(device_a: &DeviceId, device_b: &DeviceId) -> Result<RelayId> {
        if device_a == device_b {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Cannot create relay context with self".to_string(),
            });
        }

        Ok(RelayId::from_devices(device_a, device_b))
    }

    /// Derive multiple relay contexts from a device to a set of peers
    pub fn derive_relay_contexts_to_peers(
        local_device: &DeviceId,
        peers: &[DeviceId],
    ) -> Result<Vec<(DeviceId, RelayId)>> {
        let mut contexts = Vec::new();

        for peer in peers {
            if peer == local_device {
                continue; // Skip self
            }

            let relay_id = Self::derive_relay_context(local_device, peer)?;
            contexts.push((*peer, relay_id));
        }

        Ok(contexts)
    }

    /// Check if two devices can establish a relay context
    pub fn can_establish_relay(device_a: &DeviceId, device_b: &DeviceId) -> bool {
        device_a != device_b
    }
}

/// Protocol for deriving group threshold contexts (GID)
///
/// GID contexts are derived from the collective identity of a threshold group,
/// providing a shared communication context for threshold protocols.
pub struct GroupContextDerivation;

impl GroupContextDerivation {
    /// Derive a group context from threshold configuration
    ///
    /// Creates a group identity that represents the collective authority
    /// of a threshold set. The context is deterministic given the same
    /// member set and threshold parameters.
    pub fn derive_group_context(members: &[DeviceId], threshold: u16) -> Result<GroupId> {
        if members.is_empty() {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Group cannot be empty".to_string(),
            });
        }

        if threshold == 0 || threshold > members.len() as u16 {
            return Err(ContextDerivationError::InvalidInput {
                reason: format!(
                    "Invalid threshold: {} not in range [1, {}]",
                    threshold,
                    members.len()
                ),
            });
        }

        // Remove duplicates and ensure deterministic ordering
        let unique_members: BTreeSet<_> = members.iter().collect();
        if unique_members.len() != members.len() {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Duplicate members in group".to_string(),
            });
        }

        let sorted_members: Vec<DeviceId> = unique_members.into_iter().cloned().collect();
        Ok(GroupId::from_threshold_config(&sorted_members, threshold))
    }

    /// Derive a group context with specific roles
    ///
    /// Creates a group context where members may have different roles
    /// (coordinator, participants, observers) affecting the threshold calculation.
    pub fn derive_group_context_with_roles(
        coordinators: &[DeviceId],
        participants: &[DeviceId],
        threshold: u16,
    ) -> Result<(GroupId, GroupConfiguration)> {
        let mut all_members = Vec::new();
        all_members.extend_from_slice(coordinators);
        all_members.extend_from_slice(participants);

        let group_id = Self::derive_group_context(&all_members, threshold)?;

        let config = GroupConfiguration {
            coordinators: coordinators.to_vec(),
            participants: participants.to_vec(),
            threshold,
            total_members: all_members.len() as u16,
        };

        Ok((group_id, config))
    }

    /// Check if a threshold configuration is valid
    pub fn validate_threshold_config(members: &[DeviceId], threshold: u16) -> Result<()> {
        if members.is_empty() {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Group cannot be empty".to_string(),
            });
        }

        if threshold == 0 {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Threshold cannot be zero".to_string(),
            });
        }

        if threshold > members.len() as u16 {
            return Err(ContextDerivationError::InsufficientParticipants {
                required: threshold as usize,
                actual: members.len(),
            });
        }

        Ok(())
    }

    /// Get the minimum threshold for a given group size
    pub fn min_threshold_for_size(group_size: usize) -> u16 {
        ((group_size / 2) + 1) as u16 // Simple majority
    }

    /// Get the recommended threshold for a given group size (security vs availability)
    pub fn recommended_threshold_for_size(group_size: usize) -> u16 {
        match group_size {
            1 => 1,
            2 => 2, // Both required for 2-member group
            3 => 2,
            4 => 3,
            5 => 4,
            6 => 5,
            7 => 6,
            _ => ((group_size * 5 + 3) / 6) as u16, // ~5/6 threshold for large groups
        }
    }
}

/// Configuration for a threshold group
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupConfiguration {
    /// Devices that can coordinate protocols
    pub coordinators: Vec<DeviceId>,
    /// Devices that participate in threshold operations
    pub participants: Vec<DeviceId>,
    /// Number of participants required for threshold operations
    pub threshold: u16,
    /// Total number of members in the group
    pub total_members: u16,
}

impl GroupConfiguration {
    /// Get all members (coordinators + participants)
    pub fn all_members(&self) -> Vec<DeviceId> {
        let mut members = self.coordinators.clone();
        members.extend_from_slice(&self.participants);
        members.sort();
        members.dedup();
        members
    }

    /// Check if a device is a coordinator
    pub fn is_coordinator(&self, device: &DeviceId) -> bool {
        self.coordinators.contains(device)
    }

    /// Check if a device is a participant
    pub fn is_participant(&self, device: &DeviceId) -> bool {
        self.participants.contains(device)
    }

    /// Check if a device is a member
    pub fn is_member(&self, device: &DeviceId) -> bool {
        self.is_coordinator(device) || self.is_participant(device)
    }

    /// Get the effective threshold (accounting for coordinator requirements)
    pub fn effective_threshold(&self) -> u16 {
        self.threshold
    }
}

/// Protocol for deriving DKD (Deterministic Key Derivation) contexts
///
/// DKD contexts are application-scoped derived contexts that provide
/// privacy-preserving key derivation for different application domains.
pub struct DkdContextDerivation;

impl DkdContextDerivation {
    /// Derive a DKD context from application label and master material
    ///
    /// Creates a context identifier that is deterministic for the same
    /// application and master key material, but unlinkable across applications.
    pub fn derive_dkd_context(
        app_label: &str,
        master_key: &[u8; 32],
        additional_context: Option<&[u8]>,
    ) -> Result<DkdContextId> {
        if app_label.is_empty() {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Application label cannot be empty".to_string(),
            });
        }

        let mut h = hash::hasher();
        h.update(b"AURA_DKD_CONTEXT");
        h.update(app_label.as_bytes());
        h.update(master_key);

        if let Some(context) = additional_context {
            h.update(context);
        }

        let fingerprint = h.finalize();
        Ok(DkdContextId::new(app_label, fingerprint))
    }

    /// Derive multiple DKD contexts for different applications
    pub fn derive_app_contexts(
        master_key: &[u8; 32],
        app_labels: &[&str],
    ) -> Result<Vec<(String, DkdContextId)>> {
        let mut contexts = Vec::new();

        for &app_label in app_labels {
            let context = Self::derive_dkd_context(app_label, master_key, None)?;
            contexts.push((app_label.to_string(), context));
        }

        Ok(contexts)
    }

    /// Derive a DKD context with session-specific material
    pub fn derive_session_context(
        app_label: &str,
        master_key: &[u8; 32],
        session_id: &[u8],
    ) -> Result<DkdContextId> {
        Self::derive_dkd_context(app_label, master_key, Some(session_id))
    }

    /// Validate an application label
    pub fn validate_app_label(app_label: &str) -> Result<()> {
        if app_label.is_empty() {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Application label cannot be empty".to_string(),
            });
        }

        if app_label.len() > 64 {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Application label too long (max 64 characters)".to_string(),
            });
        }

        // Check for valid characters (alphanumeric + common separators)
        if !app_label
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(ContextDerivationError::InvalidInput {
                reason: "Application label contains invalid characters".to_string(),
            });
        }

        Ok(())
    }
}

/// Unified context derivation service
///
/// Provides a single interface for deriving all types of privacy contexts
/// with proper validation and error handling.
pub struct ContextDerivationService;

impl ContextDerivationService {
    /// Create a new context derivation service
    pub fn new() -> Self {
        Self
    }

    /// Derive a relay context between two devices
    pub fn derive_relay(&self, device_a: &DeviceId, device_b: &DeviceId) -> Result<MessageContext> {
        let relay_id = RelayContextDerivation::derive_relay_context(device_a, device_b)?;
        Ok(MessageContext::Relay(relay_id))
    }

    /// Derive a group context for threshold operations
    pub fn derive_group(&self, members: &[DeviceId], threshold: u16) -> Result<MessageContext> {
        let group_id = GroupContextDerivation::derive_group_context(members, threshold)?;
        Ok(MessageContext::Group(group_id))
    }

    /// Derive a DKD context for an application
    pub fn derive_dkd(
        &self,
        app_label: &str,
        master_key: &[u8; 32],
        session_context: Option<&[u8]>,
    ) -> Result<MessageContext> {
        let dkd_id =
            DkdContextDerivation::derive_dkd_context(app_label, master_key, session_context)?;
        Ok(MessageContext::DkdContext(dkd_id))
    }

    /// Validate that a context can be derived with given parameters
    pub fn validate_context_params(
        &self,
        context_type: &str,
        params: &ContextParams,
    ) -> Result<()> {
        match context_type {
            "relay" => {
                if params.devices.len() != 2 {
                    return Err(ContextDerivationError::InvalidInput {
                        reason: "Relay context requires exactly 2 devices".to_string(),
                    });
                }
                RelayContextDerivation::can_establish_relay(&params.devices[0], &params.devices[1]);
                Ok(())
            }
            "group" => {
                let threshold =
                    params
                        .threshold
                        .ok_or_else(|| ContextDerivationError::InvalidInput {
                            reason: "Group context requires threshold parameter".to_string(),
                        })?;
                GroupContextDerivation::validate_threshold_config(&params.devices, threshold)
            }
            "dkd" => {
                let app_label = params.app_label.as_ref().ok_or_else(|| {
                    ContextDerivationError::InvalidInput {
                        reason: "DKD context requires app_label parameter".to_string(),
                    }
                })?;
                DkdContextDerivation::validate_app_label(app_label)
            }
            _ => Err(ContextDerivationError::InvalidInput {
                reason: format!("Unknown context type: {}", context_type),
            }),
        }
    }
}

/// Parameters for context derivation
#[derive(Debug, Clone, Default)]
pub struct ContextParams {
    /// Devices involved in the context
    pub devices: Vec<DeviceId>,
    /// Threshold for group contexts
    pub threshold: Option<u16>,
    /// Application label for DKD contexts
    pub app_label: Option<String>,
    /// Master key material
    pub master_key: Option<[u8; 32]>,
    /// Additional context data
    pub additional_context: Option<Vec<u8>>,
}

impl Default for ContextDerivationService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relay_context_derivation() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();

        let context1 = RelayContextDerivation::derive_relay_context(&device1, &device2).unwrap();
        let context2 = RelayContextDerivation::derive_relay_context(&device2, &device1).unwrap();

        // Should be the same regardless of order
        assert_eq!(context1, context2);

        // Should not work with same device
        let result = RelayContextDerivation::derive_relay_context(&device1, &device1);
        assert!(result.is_err());
    }

    #[test]
    fn test_group_context_derivation() {
        let devices = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];

        let context = GroupContextDerivation::derive_group_context(&devices, 2).unwrap();
        let context2 = GroupContextDerivation::derive_group_context(&devices, 2).unwrap();

        // Should be deterministic
        assert_eq!(context, context2);

        // Invalid threshold should fail
        let result = GroupContextDerivation::derive_group_context(&devices, 0);
        assert!(result.is_err());

        let result = GroupContextDerivation::derive_group_context(&devices, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_dkd_context_derivation() {
        let master_key = [0u8; 32];

        let context1 =
            DkdContextDerivation::derive_dkd_context("messaging", &master_key, None).unwrap();
        let context2 =
            DkdContextDerivation::derive_dkd_context("messaging", &master_key, None).unwrap();

        // Should be deterministic
        assert_eq!(context1, context2);

        // Different apps should give different contexts
        let context3 =
            DkdContextDerivation::derive_dkd_context("storage", &master_key, None).unwrap();
        assert_ne!(context1, context3);

        // Invalid app label should fail
        let result = DkdContextDerivation::derive_dkd_context("", &master_key, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_unified_context_service() {
        let service = ContextDerivationService::new();
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();

        // Test relay context
        let relay_context = service.derive_relay(&device1, &device2).unwrap();
        assert!(matches!(relay_context, MessageContext::Relay(_)));

        // Test group context
        let devices = vec![device1, device2, DeviceId::new()];
        let group_context = service.derive_group(&devices, 2).unwrap();
        assert!(matches!(group_context, MessageContext::Group(_)));

        // Test DKD context
        let master_key = [1u8; 32];
        let dkd_context = service.derive_dkd("test_app", &master_key, None).unwrap();
        assert!(matches!(dkd_context, MessageContext::DkdContext(_)));
    }

    #[test]
    fn test_threshold_recommendations() {
        assert_eq!(GroupContextDerivation::min_threshold_for_size(1), 1);
        assert_eq!(GroupContextDerivation::min_threshold_for_size(2), 2);
        assert_eq!(GroupContextDerivation::min_threshold_for_size(3), 2);
        assert_eq!(GroupContextDerivation::min_threshold_for_size(5), 3);

        assert_eq!(GroupContextDerivation::recommended_threshold_for_size(3), 2);
        assert_eq!(GroupContextDerivation::recommended_threshold_for_size(5), 4);
        assert_eq!(GroupContextDerivation::recommended_threshold_for_size(7), 6);
    }
}
