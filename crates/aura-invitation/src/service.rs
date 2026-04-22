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
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::time::PhysicalTime;
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId};
use aura_core::{CapabilityName, DeviceId};
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

/// Type of invitation
// aura-security: secret-derive-justified owner=security-refactor expires=before-release remediation=work/2.md device-enrollment variant carries encrypted setup payloads for transfer; field-level justifications track migration to secret wrappers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationType {
    /// Invitation to join a home/channel
    Channel {
        /// Home/channel identifier
        #[serde(with = "channel_id_serde")]
        home_id: ChannelId,
        /// Optional nickname suggestion (what the channel/home wants to be called)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nickname_suggestion: Option<String>,
        /// Optional bootstrap key package for provisional AMP messaging.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bootstrap: Option<ChannelBootstrapPackage>,
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
        /// Optional nickname suggestion (what the device wants to be called)
        nickname_suggestion: Option<String>,
        /// Key-rotation ceremony identifier
        ceremony_id: CeremonyId,
        /// Pending epoch created during prepare
        pending_epoch: u64,
        /// Encrypted/opaque key package for the invited device
        // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md encrypted enrollment payload; plaintext key packages must use secret wrappers before wrapping.
        key_package: Vec<u8>,
        /// Serialized threshold config metadata for the pending epoch
        // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md enrollment ceremony metadata until envelope payloads move to SecretBytes.
        threshold_config: Vec<u8>,
        /// Untrusted key material: pending-epoch enrollment payload; authentication must resolve expected keys from trusted authority/device state.
        public_key_package: Vec<u8>,
        /// Baseline attested tree operations for the current authority state.
        ///
        /// Fresh invitees need these ops to materialize the pre-enrollment
        /// authority tree before applying the enrollment commit.
        baseline_tree_ops: Vec<Vec<u8>>,
    },
}

mod channel_id_serde {
    use aura_core::types::identifiers::ChannelId;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &ChannelId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ChannelId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        ChannelId::from_str(&raw).map_err(serde::de::Error::custom)
    }
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
    pub fn required_capability(&self) -> Option<CapabilityName> {
        match self {
            InvitationType::Channel { .. } => Some(InvitationCapability::Channel.as_name()),
            InvitationType::Guardian { .. } => Some(InvitationCapability::Guardian.as_name()),
            InvitationType::Contact { .. } => None,
            InvitationType::DeviceEnrollment { .. } => {
                Some(InvitationCapability::DeviceEnroll.as_name())
            }
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
    pub invitation_id: InvitationId,
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
    /// Optional sender-local nickname for the invitee on sent invitations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_nickname: Option<String>,
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
            .then(|| invitation_type.required_capability())
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
        let expires_at_ms = match Self::compute_expires_at_ms(snapshot.now_ms, expires_in_ms) {
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
            sent_at: Self::exact_time(snapshot.now_ms),
            expires_at: expires_at_ms.map(Self::exact_time),
            receiver_nickname: None,
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
    ) -> GuardOutcome {
        self.prepare_lifecycle_transition(
            snapshot,
            &InvitationCapability::Accept.as_name(),
            InvitationFact::Accepted {
                context_id: Some(snapshot.context_id),
                invitation_id: invitation_id.clone(),
                acceptor_id: snapshot.authority_id,
                accepted_at: Self::exact_time(snapshot.now_ms),
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
    ) -> GuardOutcome {
        self.prepare_lifecycle_transition(
            snapshot,
            &InvitationCapability::Decline.as_name(),
            InvitationFact::Declined {
                context_id: Some(snapshot.context_id),
                invitation_id: invitation_id.clone(),
                decliner_id: snapshot.authority_id,
                declined_at: Self::exact_time(snapshot.now_ms),
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
    ) -> GuardOutcome {
        self.prepare_lifecycle_transition(
            snapshot,
            &InvitationCapability::Cancel.as_name(),
            InvitationFact::Cancelled {
                context_id: Some(snapshot.context_id),
                invitation_id: invitation_id.clone(),
                canceller_id: snapshot.authority_id,
                cancelled_at: Self::exact_time(snapshot.now_ms),
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

        let outcome = service.prepare_accept_invitation(&snapshot, &invitation_id);

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 1);
    }

    #[test]
    fn test_prepare_decline_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();
        let invitation_id = InvitationId::new("inv-123");

        let outcome = service.prepare_decline_invitation(&snapshot, &invitation_id);

        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 1);
    }

    #[test]
    fn test_prepare_cancel_invitation_success() {
        let service = InvitationService::new(test_authority(), InvitationConfig::default());
        let snapshot = test_snapshot();
        let invitation_id = InvitationId::new("inv-123");

        let outcome = service.prepare_cancel_invitation(&snapshot, &invitation_id);

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
