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

use crate::capabilities::InvitationCapability;
use crate::facts::InvitationFact;
use crate::guards::{
    check_capability, check_flow_budget, costs, EffectCommand, GuardOutcome, GuardSnapshot,
};
use crate::InvitationOperation;
pub use aura_core::invitation::{Invitation, InvitationStatus, InvitationType};
use aura_core::time::{CausalMetadata, PhysicalTime};
#[cfg(test)]
use aura_core::types::identifiers::ChannelId;
use aura_core::types::identifiers::{AuthorityId, ContextId, InvitationId};
use aura_core::CapabilityName;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
enum InvitationGuardError {
    #[error("Message too long: {length} > {max}")]
    MessageTooLong { length: u32, max: u32 },
    #[error("Message length overflows u32: {length}")]
    MessageLengthOverflow { length: u64 },
    #[error("Expiration timestamp overflow: now={now_ms}, expires_in={expires_in_ms}")]
    ExpirationOverflow { now_ms: u64, expires_in_ms: u64 },
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
    pub max_message_length: u32,

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
    max_message_length: u32,
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

/// Result of an invitation action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationResult {
    /// Whether the action succeeded
    pub success: bool,
    /// Invitation ID affected
    pub invitation_id: InvitationId,
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
    fn exact_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    fn maybe_required_type_capability(
        invitation_type: &InvitationType,
        policy: &InvitationPolicy,
    ) -> Option<CapabilityName> {
        let require_check = match invitation_type {
            InvitationType::Guardian { .. } => policy.require_guardian_capability,
            InvitationType::Channel { .. } => policy.require_channel_capability,
            InvitationType::Contact { .. } => false,
            InvitationType::DeviceEnrollment { .. } => policy.require_device_capability,
        };

        require_check
            .then(|| match invitation_type {
                InvitationType::Channel { .. } => Some(InvitationCapability::Channel.as_name()),
                InvitationType::Guardian { .. } => Some(InvitationCapability::Guardian.as_name()),
                InvitationType::Contact { .. } => None,
                InvitationType::DeviceEnrollment { .. } => {
                    Some(InvitationCapability::DeviceEnroll.as_name())
                }
            })
            .flatten()
    }

    fn compute_expires_at_ms(
        now_ms: u64,
        expires_in_ms: Option<u64>,
    ) -> Result<Option<u64>, InvitationGuardError> {
        match expires_in_ms {
            Some(ms) => {
                now_ms
                    .checked_add(ms)
                    .map(Some)
                    .ok_or(InvitationGuardError::ExpirationOverflow {
                        now_ms,
                        expires_in_ms: ms,
                    })
            }
            None => Ok(None),
        }
    }

    fn prepare_lifecycle_transition(
        &self,
        snapshot: &GuardSnapshot,
        required_capability: &CapabilityName,
        fact: InvitationFact,
    ) -> GuardOutcome {
        if let Some(outcome) = check_capability(snapshot, required_capability) {
            return outcome;
        }

        GuardOutcome::allowed(vec![EffectCommand::JournalAppend { fact }])
    }

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
        invitation_id: InvitationId,
    ) -> GuardOutcome {
        self.prepare_send_invitation_header(
            snapshot,
            (receiver_id, invitation_type, message, invitation_id),
            expires_in_ms,
            None,
        )
    }

    /// Evaluate current guards while retaining the reserved canonical creation header.
    /// Runtime reservation custody must authorize execution of the returned commands.
    pub fn prepare_reserved_send_invitation(
        &self,
        snapshot: &GuardSnapshot,
        invitation: &Invitation,
    ) -> GuardOutcome {
        if invitation.sender_id != snapshot.authority_id
            || invitation.context_id != snapshot.context_id
            || invitation.created_at > snapshot.now_ms
            || invitation.is_expired(snapshot.now_ms)
            || !invitation.is_pending()
        {
            return GuardOutcome::denied(aura_guards::types::GuardViolation::AuthorizationDenied);
        }
        self.prepare_send_invitation_header(
            snapshot,
            (
                invitation.receiver_id,
                invitation.invitation_type.clone(),
                invitation.message.clone(),
                invitation.invitation_id.clone(),
            ),
            None,
            Some(invitation),
        )
    }

    fn prepare_send_invitation_header(
        &self,
        snapshot: &GuardSnapshot,
        request: (AuthorityId, InvitationType, Option<String>, InvitationId),
        expires_in_ms: Option<u64>,
        reserved: Option<&Invitation>,
    ) -> GuardOutcome {
        let (receiver_id, invitation_type, message, invitation_id) = request;
        let policy = InvitationPolicy::for_snapshot(&self.config, snapshot);
        // Check base capability
        if let Some(outcome) = check_capability(snapshot, &InvitationCapability::Send.as_name()) {
            return outcome;
        }

        // Check type-specific capability if required
        if let Some(type_capability) =
            Self::maybe_required_type_capability(&invitation_type, &policy)
        {
            if let Some(outcome) = check_capability(snapshot, &type_capability) {
                return outcome;
            }
        }

        // Check flow budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::INVITATION_SEND_COST) {
            return outcome;
        }

        // Validate message length
        if let Some(ref msg) = message {
            let length = match u32::try_from(msg.len()) {
                Ok(length) => length,
                Err(_) => {
                    let length = u64::try_from(msg.len()).unwrap_or(u64::MAX);
                    return GuardOutcome::denied(aura_guards::types::GuardViolation::other(
                        InvitationGuardError::MessageLengthOverflow { length }.to_string(),
                    ));
                }
            };

            if length > policy.max_message_length {
                return GuardOutcome::denied(aura_guards::types::GuardViolation::other(
                    InvitationGuardError::MessageTooLong {
                        length,
                        max: policy.max_message_length,
                    }
                    .to_string(),
                ));
            }
        }

        // Calculate expiration
        let expires_at_ms = match reserved
            .map(|invitation| Ok(invitation.expires_at))
            .unwrap_or_else(|| Self::compute_expires_at_ms(snapshot.now_ms, expires_in_ms))
        {
            Ok(expires_at_ms) => expires_at_ms,
            Err(error) => {
                return GuardOutcome::denied(aura_guards::types::GuardViolation::other(
                    error.to_string(),
                ));
            }
        };

        // Create the invitation fact
        let fact = InvitationFact::Sent {
            context_id: snapshot.context_id,
            invitation_id: invitation_id.clone(),
            sender_id: snapshot.authority_id,
            receiver_id,
            invitation_type,
            sent_at: Self::exact_time(
                reserved.map_or(snapshot.now_ms, |invitation| invitation.created_at),
            ),
            expires_at: expires_at_ms.map(Self::exact_time),
            receiver_nickname: reserved.and_then(|invitation| invitation.receiver_nickname.clone()),
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
                invitation_id,
            },
            EffectCommand::RecordReceipt {
                operation: InvitationOperation::SendInvitation,
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
        invitation_id: &InvitationId,
        causal: CausalMetadata,
    ) -> GuardOutcome {
        self.prepare_lifecycle_transition(
            snapshot,
            &InvitationCapability::Accept.as_name(),
            InvitationFact::Accepted {
                context_id: Some(snapshot.context_id),
                invitation_id: invitation_id.clone(),
                acceptor_id: snapshot.authority_id,
                accepted_at: Self::exact_time(snapshot.now_ms),
                causal,
            },
        )
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
        invitation_id: &InvitationId,
        causal: CausalMetadata,
    ) -> GuardOutcome {
        self.prepare_lifecycle_transition(
            snapshot,
            &InvitationCapability::Decline.as_name(),
            InvitationFact::Declined {
                context_id: Some(snapshot.context_id),
                invitation_id: invitation_id.clone(),
                decliner_id: snapshot.authority_id,
                declined_at: Self::exact_time(snapshot.now_ms),
                causal,
            },
        )
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
        invitation_id: &InvitationId,
        causal: CausalMetadata,
    ) -> GuardOutcome {
        self.prepare_lifecycle_transition(
            snapshot,
            &InvitationCapability::Cancel.as_name(),
            InvitationFact::Cancelled {
                context_id: Some(snapshot.context_id),
                invitation_id: invitation_id.clone(),
                canceller_id: snapshot.authority_id,
                cancelled_at: Self::exact_time(snapshot.now_ms),
                causal,
            },
        )
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::FlowCost;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_receiver() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([3u8; 32])
    }

    fn full_capabilities() -> Vec<aura_guards::types::CapabilityId> {
        vec![
            InvitationCapability::Send.as_name(),
            InvitationCapability::Accept.as_name(),
            InvitationCapability::Decline.as_name(),
            InvitationCapability::Cancel.as_name(),
            InvitationCapability::Guardian.as_name(),
            InvitationCapability::Channel.as_name(),
        ]
    }

    fn test_snapshot() -> GuardSnapshot {
        GuardSnapshot::new(
            test_authority(),
            test_context(),
            FlowCost::new(100),
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
            InvitationId::new("inv-123"),
        );

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 4);
    }

    #[test]
    fn reserved_send_preserves_original_header_and_checks_current_expiry() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let mut snapshot = test_snapshot();
        snapshot.now_ms = 500;
        let invitation = Invitation {
            invitation_id: InvitationId::new("original-reservation"),
            context_id: snapshot.context_id,
            sender_id: snapshot.authority_id,
            receiver_id: test_receiver(),
            invitation_type: InvitationType::Contact { nickname: None },
            status: InvitationStatus::Pending,
            created_at: 100,
            expires_at: Some(600),
            message: None,
            receiver_nickname: Some("original nickname".into()),
        };
        let outcome = service.prepare_reserved_send_invitation(&snapshot, &invitation);
        assert!(outcome.is_allowed());
        let fact = outcome
            .effects
            .iter()
            .find_map(|effect| match effect {
                EffectCommand::JournalAppend { fact } => Some(fact),
                _ => None,
            })
            .expect("canonical invitation fact");
        match fact {
            InvitationFact::Sent {
                sent_at,
                expires_at,
                receiver_nickname,
                invitation_id,
                ..
            } => {
                assert_eq!(*sent_at, InvitationService::exact_time(100));
                assert_eq!(*expires_at, Some(InvitationService::exact_time(600)));
                assert_eq!(receiver_nickname, &invitation.receiver_nickname);
                assert_eq!(invitation_id, &invitation.invitation_id);
            }
            _ => panic!("reserved send must emit Sent"),
        }
        snapshot.now_ms = 600;
        assert!(service
            .prepare_reserved_send_invitation(&snapshot, &invitation)
            .is_denied());
        snapshot.now_ms = 99;
        assert!(service
            .prepare_reserved_send_invitation(&snapshot, &invitation)
            .is_denied());
        snapshot.now_ms = 500;
        snapshot.capabilities.clear();
        assert!(service
            .prepare_reserved_send_invitation(&snapshot, &invitation)
            .is_denied());
    }

    /// Send denied without required capability — invitation operations are
    /// capability-gated.
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
            InvitationId::new("inv-123"),
        );

        assert!(outcome.is_denied());
    }

    #[test]
    fn test_prepare_send_invitation_insufficient_budget() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let mut snapshot = test_snapshot();
        snapshot.flow_budget_remaining = FlowCost::new(0);

        let outcome = service.prepare_send_invitation(
            &snapshot,
            test_receiver(),
            InvitationType::Contact { nickname: None },
            None,
            None,
            InvitationId::new("inv-123"),
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
            InvitationId::new("inv-123"),
        );

        assert!(outcome.is_denied());
    }

    #[test]
    fn test_prepare_send_invitation_expiration_overflow() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let mut snapshot = test_snapshot();
        snapshot.now_ms = u64::MAX - 1;

        let outcome = service.prepare_send_invitation(
            &snapshot,
            test_receiver(),
            InvitationType::Contact { nickname: None },
            None,
            Some(10),
            InvitationId::new("inv-123"),
        );

        assert!(outcome.is_denied());
    }

    #[test]
    fn test_prepare_accept_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();
        let invitation_id = InvitationId::new("inv-123");

        let outcome = service.prepare_accept_invitation(
            &snapshot,
            &invitation_id,
            crate::lifecycle::invitation_outcome_causal(&aura_core::time::LogicalTime {
                vector: aura_core::time::VectorClock::new(),
                lamport: 0,
            }),
        );

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 1);
    }

    #[test]
    fn test_prepare_decline_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();
        let invitation_id = InvitationId::new("inv-123");

        let outcome = service.prepare_decline_invitation(
            &snapshot,
            &invitation_id,
            crate::lifecycle::invitation_outcome_causal(&aura_core::time::LogicalTime {
                vector: aura_core::time::VectorClock::new(),
                lamport: 0,
            }),
        );

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 1);
    }

    #[test]
    fn test_prepare_cancel_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();
        let invitation_id = InvitationId::new("inv-123");

        let outcome = service.prepare_cancel_invitation(
            &snapshot,
            &invitation_id,
            crate::lifecycle::invitation_outcome_causal(&aura_core::time::LogicalTime {
                vector: aura_core::time::VectorClock::new(),
                lamport: 0,
            }),
        );

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 1);
    }

    #[test]
    fn test_invitation_type_as_string() {
        assert_eq!(
            InvitationType::Channel {
                home_id: ChannelId::from_bytes([1u8; 32]),
                nickname_suggestion: None,
                bootstrap: None,
                home: false,
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
    fn canonical_types_retain_feature_capability_policy() {
        let mut policy =
            InvitationPolicy::for_snapshot(&InvitationConfig::default(), &test_snapshot());
        let channel = InvitationType::Channel {
            home_id: ChannelId::from_bytes([219; 32]),
            nickname_suggestion: None,
            bootstrap: None,
            home: false,
        };
        let guardian = InvitationType::Guardian {
            subject_authority: test_authority(),
        };
        let device = InvitationType::DeviceEnrollment {
            setup_binding: aura_core::invitation::DeviceEnrollmentSetupBinding {
                nonce: [5; 32],
                digest: [6; 32],
            },
            subject_authority: test_authority(),
            invitee_authority: test_receiver(),
            initiator_device_id: aura_core::DeviceId::from_bytes([220; 32]),
            device_id: aura_core::DeviceId::from_bytes([221; 32]),
            nickname_suggestion: None,
            ceremony_id: aura_core::CeremonyId::new("canonical capability policy"),
            pending_epoch: 1,
            key_package: Vec::new(),
            threshold_config: Vec::new(),
            public_key_package: Vec::new(),
            baseline_tree_ops: Vec::new(),
        };
        for (invitation, expected) in [
            (&channel, Some(InvitationCapability::Channel.as_name())),
            (&guardian, Some(InvitationCapability::Guardian.as_name())),
            (&device, Some(InvitationCapability::DeviceEnroll.as_name())),
            (&InvitationType::Contact { nickname: None }, None),
        ] {
            assert_eq!(
                InvitationService::maybe_required_type_capability(invitation, &policy),
                expected
            );
        }
        policy.require_channel_capability = false;
        policy.require_guardian_capability = false;
        policy.require_device_capability = false;
        for invitation in [&channel, &guardian, &device] {
            assert_eq!(
                InvitationService::maybe_required_type_capability(invitation, &policy),
                None
            );
        }
    }

    #[test]
    fn test_channel_invitation_type_rejects_invalid_home_id_on_decode() {
        let value = serde_json::json!({
            "Channel": {
                "home_id": "not-a-channel-id",
                "nickname_suggestion": null,
                "bootstrap": null
            }
        });
        let decoded: Result<InvitationType, _> = serde_json::from_value(value);
        assert!(decoded.is_err());
    }

    /// Invitations with expiry are expired after the deadline. Without expiry,
    /// they never expire.
    #[test]
    fn test_invitation_is_expired() {
        let inv = Invitation {
            invitation_id: InvitationId::new("inv-123"),
            context_id: test_context(),
            sender_id: test_authority(),
            receiver_id: test_receiver(),
            invitation_type: InvitationType::Contact { nickname: None },
            status: InvitationStatus::Pending,
            created_at: 1000,
            expires_at: Some(2000),
            message: None,
            receiver_nickname: None,
        };

        assert!(!inv.is_expired(1500));
        assert!(inv.is_expired(2000));
        assert!(inv.is_expired(2500));
    }

    #[test]
    fn test_invitation_no_expiry() {
        let inv = Invitation {
            invitation_id: InvitationId::new("inv-123"),
            context_id: test_context(),
            sender_id: test_authority(),
            receiver_id: test_receiver(),
            invitation_type: InvitationType::Contact { nickname: None },
            status: InvitationStatus::Pending,
            created_at: 1000,
            expires_at: None,
            message: None,
            receiver_nickname: None,
        };

        // Should never expire if no expiry set
        assert!(!inv.is_expired(1000000000));
    }
}
