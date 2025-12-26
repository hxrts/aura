//! Invitation Service
//!
//! Main coordinator for invitation operations.
//! All operations flow through the guard chain and return outcomes
//! for the caller to execute effects.
//!
//! # Architecture
//!
//! The `InvitationService` follows the same pattern as `aura-rendezvous::RendezvousService`:
//!
//! 1. Caller prepares a `GuardSnapshot` asynchronously
//! 2. Service evaluates guards synchronously, returning `GuardOutcome`
//! 3. Caller executes `EffectCommand` items asynchronously
//!
//! This separation ensures:
//! - Guard evaluation is pure and testable
//! - Effect execution is explicit and controllable
//! - No I/O happens during guard evaluation

use crate::facts::InvitationFact;
use crate::guards::{
    check_capability, check_flow_budget, costs, EffectCommand, GuardOutcome, GuardSnapshot,
};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::PhysicalTime;
use aura_core::DeviceId;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
enum InvitationGuardError {
    #[error("Message too long: {length} > {max}")]
    MessageTooLong { length: usize, max: usize },
}

// =============================================================================
// Service Configuration
// =============================================================================

/// Configuration for the invitation service
#[derive(Debug, Clone)]
pub struct InvitationConfig {
    /// Default expiration time for invitations in milliseconds
    pub default_expiration_ms: u64,

    /// Maximum message length for invitations
    pub max_message_length: usize,

    /// Whether to require explicit capability for guardian invitations
    pub require_guardian_capability: bool,

    /// Whether to require explicit capability for channel invitations
    pub require_channel_capability: bool,

    /// Whether to require explicit capability for device enrollment invitations
    pub require_device_capability: bool,
}

impl Default for InvitationConfig {
    fn default() -> Self {
        Self {
            default_expiration_ms: 7 * 24 * 60 * 60 * 1000, // 7 days
            max_message_length: 1000,
            require_guardian_capability: true,
            require_channel_capability: true,
            require_device_capability: true,
        }
    }
}

#[derive(Debug, Clone)]
struct InvitationPolicy {
    #[allow(dead_code)] // Reserved for future policy enforcement
    context_id: ContextId,
    max_message_length: usize,
    require_guardian_capability: bool,
    require_channel_capability: bool,
    require_device_capability: bool,
}

impl InvitationPolicy {
    fn for_snapshot(config: &InvitationConfig, snapshot: &GuardSnapshot) -> Self {
        Self {
            context_id: snapshot.context_id,
            max_message_length: config.max_message_length,
            require_guardian_capability: config.require_guardian_capability,
            require_channel_capability: config.require_channel_capability,
            require_device_capability: config.require_device_capability,
        }
    }
}

// =============================================================================
// Invitation Types
// =============================================================================

/// Type of invitation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationType {
    /// Invitation to join a block/channel
    Channel {
        /// Block/channel identifier
        block_id: String,
    },
    /// Invitation to become a guardian
    Guardian {
        /// Authority to guard
        subject_authority: AuthorityId,
    },
    /// Invitation to become a contact
    Contact {
        /// Optional nickname for the contact
        nickname: Option<String>,
    },

    /// Invitation to enroll a new device for an account authority.
    ///
    /// This is primarily intended for out-of-band transfer (QR/copy-paste) and
    /// carries the key-share material required for the new device to install.
    DeviceEnrollment {
        /// Account authority being modified
        subject_authority: AuthorityId,
        /// Initiator device id (used for routing acceptance back to the right device runtime)
        initiator_device_id: DeviceId,
        /// Device id being enrolled
        device_id: DeviceId,
        /// Optional device label
        device_name: Option<String>,
        /// Key-rotation ceremony identifier
        ceremony_id: String,
        /// Pending epoch created during prepare
        pending_epoch: u64,
        /// Encrypted/opaque key package for the invited device
        key_package: Vec<u8>,
    },
}

impl InvitationType {
    /// Convert to type string for fact storage
    pub fn as_type_string(&self) -> String {
        match self {
            InvitationType::Channel { .. } => "channel".to_string(),
            InvitationType::Guardian { .. } => "guardian".to_string(),
            InvitationType::Contact { .. } => "contact".to_string(),
            InvitationType::DeviceEnrollment { .. } => "device".to_string(),
        }
    }

    /// Get required capability for this invitation type (if any)
    pub fn required_capability(&self) -> Option<&'static str> {
        match self {
            InvitationType::Channel { .. } => Some(costs::CAP_CHANNEL_INVITE),
            InvitationType::Guardian { .. } => Some(costs::CAP_GUARDIAN_INVITE),
            InvitationType::Contact { .. } => None,
            InvitationType::DeviceEnrollment { .. } => Some(costs::CAP_DEVICE_ENROLL),
        }
    }
}

/// Invitation status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationStatus {
    /// Invitation is pending response
    Pending,
    /// Invitation was accepted
    Accepted,
    /// Invitation was declined
    Declined,
    /// Invitation was cancelled by sender
    Cancelled,
    /// Invitation has expired
    Expired,
}

/// Cached invitation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invitation {
    /// Unique invitation identifier
    pub invitation_id: String,
    /// Context for the invitation
    pub context_id: ContextId,
    /// Sender authority
    pub sender_id: AuthorityId,
    /// Receiver authority
    pub receiver_id: AuthorityId,
    /// Type of invitation
    pub invitation_type: InvitationType,
    /// Current status
    pub status: InvitationStatus,
    /// Creation timestamp (ms)
    pub created_at: u64,
    /// Expiration timestamp (ms), if any
    pub expires_at: Option<u64>,
    /// Optional message
    pub message: Option<String>,
}

impl Invitation {
    /// Check if invitation is expired
    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.expires_at.map(|exp| now_ms >= exp).unwrap_or(false)
    }

    /// Check if invitation is pending
    pub fn is_pending(&self) -> bool {
        matches!(self.status, InvitationStatus::Pending)
    }
}

/// Result of an invitation action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationResult {
    /// Whether the action succeeded
    pub success: bool,
    /// Invitation ID affected
    pub invitation_id: String,
    /// New status after the action
    pub new_status: Option<InvitationStatus>,
    /// Error message if action failed
    pub error: Option<String>,
}

// =============================================================================
// Invitation Service
// =============================================================================

/// Invitation service coordinating invitation operations
pub struct InvitationService {
    /// Local authority
    authority_id: AuthorityId,
    /// Service configuration
    config: InvitationConfig,
}

impl InvitationService {
    /// Create a new invitation service
    pub fn new(authority_id: AuthorityId, config: InvitationConfig) -> Self {
        Self {
            authority_id,
            config,
        }
    }

    /// Get the local authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get the service configuration
    pub fn config(&self) -> &InvitationConfig {
        &self.config
    }

    // =========================================================================
    // Send Invitation
    // =========================================================================

    /// Prepare to send an invitation.
    ///
    /// Returns a `GuardOutcome` that the caller must evaluate and execute.
    pub fn prepare_send_invitation(
        &self,
        snapshot: &GuardSnapshot,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        message: Option<String>,
        expires_in_ms: Option<u64>,
        invitation_id: String,
    ) -> GuardOutcome {
        let policy = InvitationPolicy::for_snapshot(&self.config, snapshot);
        // Check base capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_INVITATION_SEND) {
            return outcome;
        }

        // Check type-specific capability if required
        if let Some(type_cap) = invitation_type.required_capability() {
            let require_check = match &invitation_type {
                InvitationType::Guardian { .. } => policy.require_guardian_capability,
                InvitationType::Channel { .. } => policy.require_channel_capability,
                InvitationType::Contact { .. } => false,
                InvitationType::DeviceEnrollment { .. } => policy.require_device_capability,
            };

            if require_check {
                if let Some(outcome) = check_capability(snapshot, type_cap) {
                    return outcome;
                }
            }
        }

        // Check flow budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::INVITATION_SEND_COST) {
            return outcome;
        }

        // Validate message length
        if let Some(ref msg) = message {
            if msg.len() > policy.max_message_length {
                return GuardOutcome::denied(
                    InvitationGuardError::MessageTooLong {
                        length: msg.len(),
                        max: policy.max_message_length,
                    }
                    .to_string(),
                );
            }
        }

        // Calculate expiration
        let expires_at_ms = expires_in_ms.map(|ms| snapshot.now_ms + ms);

        // Create the invitation fact
        let fact = InvitationFact::Sent {
            context_id: snapshot.context_id,
            invitation_id: invitation_id.clone(),
            sender_id: snapshot.authority_id,
            receiver_id,
            invitation_type: invitation_type.as_type_string(),
            sent_at: PhysicalTime {
                ts_ms: snapshot.now_ms,
                uncertainty: None,
            },
            expires_at: expires_at_ms.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
            message,
        };

        // Construct effect commands
        let effects = vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::INVITATION_SEND_COST,
            },
            EffectCommand::JournalAppend { fact },
            EffectCommand::NotifyPeer {
                peer: receiver_id,
                invitation_id: invitation_id.clone(),
            },
            EffectCommand::RecordReceipt {
                operation: "send_invitation".to_string(),
                peer: Some(receiver_id),
            },
        ];

        GuardOutcome::allowed(effects)
    }

    // =========================================================================
    // Accept Invitation
    // =========================================================================

    /// Prepare to accept an invitation.
    ///
    /// Returns a `GuardOutcome` that the caller must evaluate and execute.
    pub fn prepare_accept_invitation(
        &self,
        snapshot: &GuardSnapshot,
        invitation_id: &str,
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_INVITATION_ACCEPT) {
            return outcome;
        }

        // Check flow budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::INVITATION_ACCEPT_COST) {
            return outcome;
        }

        // Create acceptance fact
        let fact = InvitationFact::Accepted {
            invitation_id: invitation_id.to_string(),
            acceptor_id: snapshot.authority_id,
            accepted_at: PhysicalTime {
                ts_ms: snapshot.now_ms,
                uncertainty: None,
            },
        };

        // Construct effect commands
        let effects = vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::INVITATION_ACCEPT_COST,
            },
            EffectCommand::JournalAppend { fact },
            EffectCommand::RecordReceipt {
                operation: "accept_invitation".to_string(),
                peer: None,
            },
        ];

        GuardOutcome::allowed(effects)
    }

    // =========================================================================
    // Decline Invitation
    // =========================================================================

    /// Prepare to decline an invitation.
    ///
    /// Returns a `GuardOutcome` that the caller must evaluate and execute.
    pub fn prepare_decline_invitation(
        &self,
        snapshot: &GuardSnapshot,
        invitation_id: &str,
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_INVITATION_DECLINE) {
            return outcome;
        }

        // Check flow budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::INVITATION_DECLINE_COST) {
            return outcome;
        }

        // Create decline fact
        let fact = InvitationFact::Declined {
            invitation_id: invitation_id.to_string(),
            decliner_id: snapshot.authority_id,
            declined_at: PhysicalTime {
                ts_ms: snapshot.now_ms,
                uncertainty: None,
            },
        };

        // Construct effect commands
        let effects = vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::INVITATION_DECLINE_COST,
            },
            EffectCommand::JournalAppend { fact },
            EffectCommand::RecordReceipt {
                operation: "decline_invitation".to_string(),
                peer: None,
            },
        ];

        GuardOutcome::allowed(effects)
    }

    // =========================================================================
    // Cancel Invitation
    // =========================================================================

    /// Prepare to cancel an invitation (sender only).
    ///
    /// Returns a `GuardOutcome` that the caller must evaluate and execute.
    pub fn prepare_cancel_invitation(
        &self,
        snapshot: &GuardSnapshot,
        invitation_id: &str,
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_INVITATION_CANCEL) {
            return outcome;
        }

        // Check flow budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::INVITATION_CANCEL_COST) {
            return outcome;
        }

        // Create cancellation fact
        let fact = InvitationFact::Cancelled {
            invitation_id: invitation_id.to_string(),
            canceller_id: snapshot.authority_id,
            cancelled_at: PhysicalTime {
                ts_ms: snapshot.now_ms,
                uncertainty: None,
            },
        };

        // Construct effect commands
        let effects = vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::INVITATION_CANCEL_COST,
            },
            EffectCommand::JournalAppend { fact },
            EffectCommand::RecordReceipt {
                operation: "cancel_invitation".to_string(),
                peer: None,
            },
        ];

        GuardOutcome::allowed(effects)
    }

}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_receiver() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([3u8; 32])
    }

    fn full_capabilities() -> Vec<String> {
        vec![
            costs::CAP_INVITATION_SEND.to_string(),
            costs::CAP_INVITATION_ACCEPT.to_string(),
            costs::CAP_INVITATION_DECLINE.to_string(),
            costs::CAP_INVITATION_CANCEL.to_string(),
            costs::CAP_GUARDIAN_INVITE.to_string(),
            costs::CAP_CHANNEL_INVITE.to_string(),
        ]
    }

    fn test_snapshot() -> GuardSnapshot {
        GuardSnapshot::new(
            test_authority(),
            test_context(),
            100,
            full_capabilities(),
            1,
            1000,
        )
    }

    #[test]
    fn test_service_creation() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        assert_eq!(service.authority_id(), test_authority());
    }

    #[test]
    fn test_prepare_send_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();

        let outcome = service.prepare_send_invitation(
            &snapshot,
            test_receiver(),
            InvitationType::Contact { nickname: None },
            Some("Hello!".to_string()),
            Some(86400000),
            "inv-123".to_string(),
        );

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 4);
    }

    #[test]
    fn test_prepare_send_invitation_missing_capability() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let mut snapshot = test_snapshot();
        snapshot.capabilities.clear();

        let outcome = service.prepare_send_invitation(
            &snapshot,
            test_receiver(),
            InvitationType::Contact { nickname: None },
            None,
            None,
            "inv-123".to_string(),
        );

        assert!(outcome.is_denied());
    }

    #[test]
    fn test_prepare_send_invitation_insufficient_budget() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let mut snapshot = test_snapshot();
        snapshot.flow_budget_remaining = 0;

        let outcome = service.prepare_send_invitation(
            &snapshot,
            test_receiver(),
            InvitationType::Contact { nickname: None },
            None,
            None,
            "inv-123".to_string(),
        );

        assert!(outcome.is_denied());
    }

    #[test]
    fn test_prepare_send_invitation_message_too_long() {
        let config = InvitationConfig {
            max_message_length: 10,
            ..Default::default()
        };
        let service = InvitationService::new(test_authority(), config);
        let snapshot = test_snapshot();

        let outcome = service.prepare_send_invitation(
            &snapshot,
            test_receiver(),
            InvitationType::Contact { nickname: None },
            Some("This message is way too long for the limit".to_string()),
            None,
            "inv-123".to_string(),
        );

        assert!(outcome.is_denied());
    }

    #[test]
    fn test_prepare_accept_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();

        let outcome = service.prepare_accept_invitation(&snapshot, "inv-123");

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 3);
    }

    #[test]
    fn test_prepare_decline_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();

        let outcome = service.prepare_decline_invitation(&snapshot, "inv-123");

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 3);
    }

    #[test]
    fn test_prepare_cancel_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();

        let outcome = service.prepare_cancel_invitation(&snapshot, "inv-123");

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 3);
    }

    #[test]
    fn test_invitation_type_as_string() {
        assert_eq!(
            InvitationType::Channel {
                block_id: "b".to_string()
            }
            .as_type_string(),
            "channel"
        );
        assert_eq!(
            InvitationType::Guardian {
                subject_authority: test_authority()
            }
            .as_type_string(),
            "guardian"
        );
        assert_eq!(
            InvitationType::Contact { nickname: None }.as_type_string(),
            "contact"
        );
    }

    #[test]
    fn test_invitation_is_expired() {
        let inv = Invitation {
            invitation_id: "inv-123".to_string(),
            context_id: test_context(),
            sender_id: test_authority(),
            receiver_id: test_receiver(),
            invitation_type: InvitationType::Contact { nickname: None },
            status: InvitationStatus::Pending,
            created_at: 1000,
            expires_at: Some(2000),
            message: None,
        };

        assert!(!inv.is_expired(1500));
        assert!(inv.is_expired(2000));
        assert!(inv.is_expired(2500));
    }

    #[test]
    fn test_invitation_no_expiry() {
        let inv = Invitation {
            invitation_id: "inv-123".to_string(),
            context_id: test_context(),
            sender_id: test_authority(),
            receiver_id: test_receiver(),
            invitation_type: InvitationType::Contact { nickname: None },
            status: InvitationStatus::Pending,
            created_at: 1000,
            expires_at: None,
            message: None,
        };

        // Should never expire if no expiry set
        assert!(!inv.is_expired(1000000000));
    }
}
