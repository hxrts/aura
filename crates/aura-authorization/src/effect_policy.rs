//! Effect Policy Infrastructure
//!
//! Defines when and how operation effects are applied, independent of authorization.
//! This separates two orthogonal concerns:
//!
//! - **Authorization** (capability system): Can you perform this operation?
//! - **Effect Timing** (policy system): When is the effect applied?
//!
//! # Design Principles
//!
//! 1. **Capabilities remain pure authorization** - they answer "can you do this?"
//! 2. **Effect policies determine timing** - they answer "when does the effect apply?"
//! 3. **Policies reference capabilities** - approval requirements use capability holders
//! 4. **Context-specific overrides** - channels/groups can customize their policies
//!
//! # Effect Timing Categories
//!
//! - **Immediate**: Effect applied locally, syncs in background (optimistic)
//! - **Deferred**: Effect waits for agreement, shows "pending" in UI
//! - **Blocking**: User waits for ceremony to complete
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_authorization::effect_policy::{EffectPolicyRegistry, OperationType, EffectTiming};
//!
//! let registry = EffectPolicyRegistry::default();
//!
//! // Check policy for an operation
//! let timing = registry.get_timing(&OperationType::SendMessage, &context_id);
//! match timing {
//!     EffectTiming::Immediate => apply_immediately(),
//!     EffectTiming::Deferred { .. } => create_proposal(),
//!     EffectTiming::Blocking { .. } => run_ceremony(),
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use aura_core::ContextId;

/// High-level operation types for effect policy classification
///
/// These are distinct from `ResourceScope` operations - they represent
/// user-facing actions rather than authorization primitives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationType {
    // === Message Operations ===
    /// Send a message within a channel
    SendMessage,
    /// Edit own message
    EditMessage,
    /// Delete own message
    DeleteMessage,
    /// React to a message
    ReactToMessage,

    // === Channel Operations ===
    /// Create a new channel within an existing context
    CreateChannel,
    /// Update channel topic/description
    UpdateChannelTopic,
    /// Archive a channel
    ArchiveChannel,
    /// Delete a channel
    DeleteChannel,
    /// Pin/unpin a message
    PinMessage,

    // === Channel Membership ===
    /// Add member to channel (within same context)
    AddChannelMember,
    /// Remove member from channel
    RemoveChannelMember,
    /// Change channel permissions/roles
    ChangeChannelPermissions,
    /// Transfer channel ownership
    TransferChannelOwnership,

    // === Contact/Relationship Operations ===
    /// Add a new contact (establishes relationship)
    AddContact,
    /// Block a contact
    BlockContact,
    /// Unblock a contact
    UnblockContact,
    /// Set display name for contact
    SetContactNickname,

    // === Group Operations ===
    /// Create a multi-party group
    CreateGroup,
    /// Add member to group (crypto context change)
    AddGroupMember,
    /// Remove member from group
    RemoveGroupMember,

    // === Profile Operations ===
    /// Update own profile
    UpdateProfile,
    /// Update notification preferences
    UpdatePreferences,

    // === Guardian/Recovery Operations ===
    /// Rotate guardian set
    RotateGuardians,
    /// Execute recovery
    ExecuteRecovery,
    /// Approve recovery request
    ApproveRecovery,

    // === Device Operations ===
    /// Add device to authority
    AddDevice,
    /// Revoke device
    RevokeDevice,

    // === OTA Operations ===
    /// Propose OTA update
    ProposeOTAUpdate,
    /// Activate OTA hard fork
    ActivateOTA,

    // === Social Home Operations ===
    /// Join a social home
    JoinSocialBlock,
    /// Propose home adjacency
    ProposeBlockAdjacency,
}

impl OperationType {
    /// Get the string identifier for this operation type
    pub fn as_str(&self) -> &'static str {
        match self {
            OperationType::SendMessage => "send_message",
            OperationType::EditMessage => "edit_message",
            OperationType::DeleteMessage => "delete_message",
            OperationType::ReactToMessage => "react_to_message",
            OperationType::CreateChannel => "create_channel",
            OperationType::UpdateChannelTopic => "update_channel_topic",
            OperationType::ArchiveChannel => "archive_channel",
            OperationType::DeleteChannel => "delete_channel",
            OperationType::PinMessage => "pin_message",
            OperationType::AddChannelMember => "add_channel_member",
            OperationType::RemoveChannelMember => "remove_channel_member",
            OperationType::ChangeChannelPermissions => "change_channel_permissions",
            OperationType::TransferChannelOwnership => "transfer_channel_ownership",
            OperationType::AddContact => "add_contact",
            OperationType::BlockContact => "block_contact",
            OperationType::UnblockContact => "unblock_contact",
            OperationType::SetContactNickname => "set_contact_nickname",
            OperationType::CreateGroup => "create_group",
            OperationType::AddGroupMember => "add_group_member",
            OperationType::RemoveGroupMember => "remove_group_member",
            OperationType::UpdateProfile => "update_profile",
            OperationType::UpdatePreferences => "update_preferences",
            OperationType::RotateGuardians => "rotate_guardians",
            OperationType::ExecuteRecovery => "execute_recovery",
            OperationType::ApproveRecovery => "approve_recovery",
            OperationType::AddDevice => "add_device",
            OperationType::RevokeDevice => "revoke_device",
            OperationType::ProposeOTAUpdate => "propose_ota_update",
            OperationType::ActivateOTA => "activate_ota",
            OperationType::JoinSocialBlock => "join_social_block",
            OperationType::ProposeBlockAdjacency => "propose_block_adjacency",
        }
    }

    /// Get the default security level for this operation
    pub fn default_security_level(&self) -> SecurityLevel {
        match self {
            // Low risk - within existing context
            OperationType::SendMessage
            | OperationType::EditMessage
            | OperationType::DeleteMessage
            | OperationType::ReactToMessage
            | OperationType::CreateChannel
            | OperationType::UpdateChannelTopic
            | OperationType::PinMessage
            | OperationType::BlockContact
            | OperationType::UnblockContact
            | OperationType::SetContactNickname
            | OperationType::UpdateProfile
            | OperationType::UpdatePreferences => SecurityLevel::Low,

            // Medium risk - affects others but reversible
            OperationType::ArchiveChannel
            | OperationType::AddChannelMember
            | OperationType::RemoveChannelMember
            | OperationType::ChangeChannelPermissions
            | OperationType::JoinSocialBlock
            | OperationType::ProposeBlockAdjacency => SecurityLevel::Medium,

            // High risk - irreversible or security-critical
            OperationType::DeleteChannel
            | OperationType::TransferChannelOwnership
            | OperationType::RemoveGroupMember => SecurityLevel::High,

            // Critical - establishes or modifies crypto relationships
            OperationType::AddContact
            | OperationType::CreateGroup
            | OperationType::AddGroupMember
            | OperationType::RotateGuardians
            | OperationType::ExecuteRecovery
            | OperationType::ApproveRecovery
            | OperationType::AddDevice
            | OperationType::RevokeDevice
            | OperationType::ProposeOTAUpdate
            | OperationType::ActivateOTA => SecurityLevel::Critical,
        }
    }
}

impl FromStr for OperationType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "send_message" => Ok(OperationType::SendMessage),
            "edit_message" => Ok(OperationType::EditMessage),
            "delete_message" => Ok(OperationType::DeleteMessage),
            "react_to_message" => Ok(OperationType::ReactToMessage),
            "create_channel" => Ok(OperationType::CreateChannel),
            "update_channel_topic" => Ok(OperationType::UpdateChannelTopic),
            "archive_channel" => Ok(OperationType::ArchiveChannel),
            "delete_channel" => Ok(OperationType::DeleteChannel),
            "pin_message" => Ok(OperationType::PinMessage),
            "add_channel_member" => Ok(OperationType::AddChannelMember),
            "remove_channel_member" => Ok(OperationType::RemoveChannelMember),
            "change_channel_permissions" => Ok(OperationType::ChangeChannelPermissions),
            "transfer_channel_ownership" => Ok(OperationType::TransferChannelOwnership),
            "add_contact" => Ok(OperationType::AddContact),
            "block_contact" => Ok(OperationType::BlockContact),
            "unblock_contact" => Ok(OperationType::UnblockContact),
            "set_contact_nickname" => Ok(OperationType::SetContactNickname),
            "create_group" => Ok(OperationType::CreateGroup),
            "add_group_member" => Ok(OperationType::AddGroupMember),
            "remove_group_member" => Ok(OperationType::RemoveGroupMember),
            "update_profile" => Ok(OperationType::UpdateProfile),
            "update_preferences" => Ok(OperationType::UpdatePreferences),
            "rotate_guardians" => Ok(OperationType::RotateGuardians),
            "execute_recovery" => Ok(OperationType::ExecuteRecovery),
            "approve_recovery" => Ok(OperationType::ApproveRecovery),
            "add_device" => Ok(OperationType::AddDevice),
            "revoke_device" => Ok(OperationType::RevokeDevice),
            "propose_ota_update" => Ok(OperationType::ProposeOTAUpdate),
            "activate_ota" => Ok(OperationType::ActivateOTA),
            "join_social_block" => Ok(OperationType::JoinSocialBlock),
            "propose_block_adjacency" => Ok(OperationType::ProposeBlockAdjacency),
            _ => Err(format!("Unknown operation type: {}", s)),
        }
    }
}

/// Security level classification for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SecurityLevel {
    /// Low risk operations (within context, personal preferences)
    Low,
    /// Medium risk (affects others, but reversible)
    Medium,
    /// High risk (irreversible or significant impact)
    High,
    /// Critical (security-sensitive, requires ceremony)
    Critical,
}

/// Determines when an operation's effect is applied
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectTiming {
    /// Effect applied immediately, syncs in background
    ///
    /// Used for low-risk operations where optimistic execution is safe.
    /// User sees result immediately; sync failures show indicator but don't revert.
    Immediate,

    /// Effect deferred until agreement is reached
    ///
    /// Used for operations where partial state is undesirable.
    /// User sees "pending" UI immediately; effect applied after approval.
    Deferred {
        /// Role(s) required to approve this operation
        requires_approval_from: Vec<CapabilityRequirement>,
        /// Timeout in milliseconds before auto-rejection
        timeout_ms: u64,
        /// How many approvals are needed
        threshold: ApprovalThreshold,
    },

    /// Effect blocked until ceremony completes
    ///
    /// Used for operations that establish or modify cryptographic relationships.
    /// User waits with progress indicator until ceremony finishes.
    Blocking {
        /// Type of ceremony required
        ceremony: CeremonyType,
    },
}

impl EffectTiming {
    /// Create an immediate timing policy
    pub fn immediate() -> Self {
        EffectTiming::Immediate
    }

    /// Create a deferred timing policy with single admin approval
    pub fn deferred_single_admin(timeout_hours: u64) -> Self {
        EffectTiming::Deferred {
            requires_approval_from: vec![CapabilityRequirement::Role("admin".to_string())],
            timeout_ms: timeout_hours * 60 * 60 * 1000,
            threshold: ApprovalThreshold::Any,
        }
    }

    /// Create a deferred timing policy with unanimous admin approval
    pub fn deferred_unanimous_admin(timeout_hours: u64) -> Self {
        EffectTiming::Deferred {
            requires_approval_from: vec![CapabilityRequirement::Role("admin".to_string())],
            timeout_ms: timeout_hours * 60 * 60 * 1000,
            threshold: ApprovalThreshold::Unanimous,
        }
    }

    /// Create a blocking timing policy
    pub fn blocking(ceremony: CeremonyType) -> Self {
        EffectTiming::Blocking { ceremony }
    }

    /// Check if this timing is immediate
    pub fn is_immediate(&self) -> bool {
        matches!(self, EffectTiming::Immediate)
    }

    /// Check if this timing is deferred
    pub fn is_deferred(&self) -> bool {
        matches!(self, EffectTiming::Deferred { .. })
    }

    /// Check if this timing is blocking
    pub fn is_blocking(&self) -> bool {
        matches!(self, EffectTiming::Blocking { .. })
    }
}

/// Requirement for approval capability
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityRequirement {
    /// Must hold a specific role (e.g., "admin", "owner")
    Role(String),
    /// Must be a specific authority
    Authority(String),
    /// Must be the initiator of the operation
    Initiator,
    /// Any member of the context
    AnyMember,
}

/// How many approvals are required
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalThreshold {
    /// Any single holder of the required capability
    Any,
    /// All holders must approve
    Unanimous,
    /// K-of-N threshold approval
    Threshold {
        /// Minimum approvals required
        required: u32,
    },
    /// Percentage of holders must approve
    Percentage {
        /// Percentage required (0-100)
        percent: u8,
    },
}

impl ApprovalThreshold {
    /// Check if this threshold is met given approvals and total eligible
    pub fn is_met(&self, approvals: u32, total_eligible: u32) -> bool {
        match self {
            ApprovalThreshold::Any => approvals >= 1,
            ApprovalThreshold::Unanimous => approvals >= total_eligible,
            ApprovalThreshold::Threshold { required } => approvals >= *required,
            ApprovalThreshold::Percentage { percent } => {
                let required = (total_eligible * (*percent as u32)).div_ceil(100);
                approvals >= required
            }
        }
    }
}

/// Types of ceremonies for blocking operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CeremonyType {
    /// Invitation acceptance ceremony (contact/channel)
    Invitation,
    /// Guardian rotation ceremony
    GuardianRotation,
    /// Recovery execution ceremony
    Recovery,
    /// OTA upgrade activation ceremony
    OTAActivation,
    /// Group membership change ceremony
    GroupMembership,
}

impl CeremonyType {
    /// Get string identifier for this ceremony type
    pub fn as_str(&self) -> &'static str {
        match self {
            CeremonyType::Invitation => "invitation",
            CeremonyType::GuardianRotation => "guardian_rotation",
            CeremonyType::Recovery => "recovery",
            CeremonyType::OTAActivation => "ota_activation",
            CeremonyType::GroupMembership => "group_membership",
        }
    }
}

/// A complete effect policy for an operation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectPolicy {
    /// The operation this policy applies to
    pub operation: OperationType,
    /// When the effect is applied
    pub timing: EffectTiming,
    /// Security level (for UI hints)
    pub security_level: SecurityLevel,
}

impl EffectPolicy {
    /// Create a new effect policy
    pub fn new(operation: OperationType, timing: EffectTiming) -> Self {
        let security_level = operation.default_security_level();
        Self {
            operation,
            timing,
            security_level,
        }
    }

    /// Create a policy with explicit security level
    pub fn with_security_level(
        operation: OperationType,
        timing: EffectTiming,
        security_level: SecurityLevel,
    ) -> Self {
        Self {
            operation,
            timing,
            security_level,
        }
    }
}

/// Key for context-specific policy overrides
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PolicyKey {
    context_id: Option<ContextId>,
    operation: OperationType,
}

/// Registry of effect policies with defaults and overrides
#[derive(Debug, Clone)]
pub struct EffectPolicyRegistry {
    /// Default policies by operation type
    defaults: HashMap<OperationType, EffectTiming>,
    /// Context-specific overrides (context_id, operation) -> timing
    overrides: HashMap<PolicyKey, EffectTiming>,
}

impl EffectPolicyRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            defaults: HashMap::new(),
            overrides: HashMap::new(),
        }
    }

    /// Set a default policy for an operation type
    pub fn set_default(&mut self, operation: OperationType, timing: EffectTiming) {
        self.defaults.insert(operation, timing);
    }

    /// Set a context-specific override
    pub fn set_override(
        &mut self,
        context_id: ContextId,
        operation: OperationType,
        timing: EffectTiming,
    ) {
        let key = PolicyKey {
            context_id: Some(context_id),
            operation,
        };
        self.overrides.insert(key, timing);
    }

    /// Remove a context-specific override
    pub fn remove_override(&mut self, context_id: &ContextId, operation: &OperationType) {
        let key = PolicyKey {
            context_id: Some(*context_id),
            operation: operation.clone(),
        };
        self.overrides.remove(&key);
    }

    /// Get the effective timing for an operation
    ///
    /// Resolution order:
    /// 1. Context-specific override (if context_id provided)
    /// 2. Default for operation type
    /// 3. Fallback based on security level
    pub fn get_timing(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> EffectTiming {
        // Check context-specific override first
        if let Some(ctx_id) = context_id {
            let key = PolicyKey {
                context_id: Some(*ctx_id),
                operation: operation.clone(),
            };
            if let Some(timing) = self.overrides.get(&key) {
                return timing.clone();
            }
        }

        // Check default for this operation
        if let Some(timing) = self.defaults.get(operation) {
            return timing.clone();
        }

        // Fallback based on security level
        Self::fallback_timing(operation)
    }

    /// Get full policy including security level
    pub fn get_policy(
        &self,
        operation: &OperationType,
        context_id: Option<&ContextId>,
    ) -> EffectPolicy {
        let timing = self.get_timing(operation, context_id);
        EffectPolicy {
            operation: operation.clone(),
            timing,
            security_level: operation.default_security_level(),
        }
    }

    /// Default fallback timing based on security level
    fn fallback_timing(operation: &OperationType) -> EffectTiming {
        match operation.default_security_level() {
            SecurityLevel::Low => EffectTiming::Immediate,
            SecurityLevel::Medium => EffectTiming::Deferred {
                requires_approval_from: vec![CapabilityRequirement::Role("admin".to_string())],
                timeout_ms: 24 * 60 * 60 * 1000, // 24 hours
                threshold: ApprovalThreshold::Any,
            },
            SecurityLevel::High => EffectTiming::Deferred {
                requires_approval_from: vec![CapabilityRequirement::Role("admin".to_string())],
                timeout_ms: 24 * 60 * 60 * 1000,
                threshold: ApprovalThreshold::Unanimous,
            },
            SecurityLevel::Critical => EffectTiming::Blocking {
                ceremony: CeremonyType::Invitation, // Generic fallback
            },
        }
    }
}

impl Default for EffectPolicyRegistry {
    /// Create a registry with sensible defaults for all operation types
    fn default() -> Self {
        let mut registry = Self::new();

        // === LOW RISK: Immediate ===
        // Within-context operations that are safe to apply optimistically
        registry.set_default(OperationType::SendMessage, EffectTiming::Immediate);
        registry.set_default(OperationType::EditMessage, EffectTiming::Immediate);
        registry.set_default(OperationType::DeleteMessage, EffectTiming::Immediate);
        registry.set_default(OperationType::ReactToMessage, EffectTiming::Immediate);
        registry.set_default(OperationType::CreateChannel, EffectTiming::Immediate);
        registry.set_default(OperationType::UpdateChannelTopic, EffectTiming::Immediate);
        registry.set_default(OperationType::PinMessage, EffectTiming::Immediate);
        registry.set_default(OperationType::BlockContact, EffectTiming::Immediate);
        registry.set_default(OperationType::UnblockContact, EffectTiming::Immediate);
        registry.set_default(OperationType::SetContactNickname, EffectTiming::Immediate);
        registry.set_default(OperationType::UpdateProfile, EffectTiming::Immediate);
        registry.set_default(OperationType::UpdatePreferences, EffectTiming::Immediate);

        // === MEDIUM RISK: Deferred with single admin approval ===
        // Affects others but is reversible
        registry.set_default(
            OperationType::ArchiveChannel,
            EffectTiming::deferred_single_admin(24),
        );
        registry.set_default(
            OperationType::AddChannelMember,
            EffectTiming::deferred_single_admin(24),
        );
        registry.set_default(
            OperationType::RemoveChannelMember,
            EffectTiming::deferred_single_admin(24),
        );
        registry.set_default(
            OperationType::ChangeChannelPermissions,
            EffectTiming::deferred_single_admin(24),
        );
        registry.set_default(
            OperationType::JoinSocialBlock,
            EffectTiming::deferred_single_admin(48),
        );
        registry.set_default(
            OperationType::ProposeBlockAdjacency,
            EffectTiming::deferred_single_admin(48),
        );

        // === HIGH RISK: Deferred with unanimous admin approval ===
        // Irreversible or significant security impact
        registry.set_default(
            OperationType::DeleteChannel,
            EffectTiming::deferred_unanimous_admin(24),
        );
        registry.set_default(
            OperationType::TransferChannelOwnership,
            EffectTiming::Deferred {
                requires_approval_from: vec![CapabilityRequirement::Role("owner".to_string())],
                timeout_ms: 7 * 24 * 60 * 60 * 1000, // 7 days
                threshold: ApprovalThreshold::Unanimous,
            },
        );
        registry.set_default(
            OperationType::RemoveGroupMember,
            EffectTiming::deferred_unanimous_admin(24),
        );

        // === CRITICAL: Blocking with ceremony ===
        // Establishes or modifies cryptographic relationships
        registry.set_default(
            OperationType::AddContact,
            EffectTiming::blocking(CeremonyType::Invitation),
        );
        registry.set_default(
            OperationType::CreateGroup,
            EffectTiming::blocking(CeremonyType::GroupMembership),
        );
        registry.set_default(
            OperationType::AddGroupMember,
            EffectTiming::blocking(CeremonyType::GroupMembership),
        );
        registry.set_default(
            OperationType::RotateGuardians,
            EffectTiming::blocking(CeremonyType::GuardianRotation),
        );
        registry.set_default(
            OperationType::ExecuteRecovery,
            EffectTiming::blocking(CeremonyType::Recovery),
        );
        registry.set_default(
            OperationType::ApproveRecovery,
            EffectTiming::blocking(CeremonyType::Recovery),
        );
        registry.set_default(
            OperationType::AddDevice,
            EffectTiming::blocking(CeremonyType::Invitation),
        );
        registry.set_default(
            OperationType::RevokeDevice,
            EffectTiming::blocking(CeremonyType::GuardianRotation),
        );
        registry.set_default(
            OperationType::ProposeOTAUpdate,
            EffectTiming::blocking(CeremonyType::OTAActivation),
        );
        registry.set_default(
            OperationType::ActivateOTA,
            EffectTiming::blocking(CeremonyType::OTAActivation),
        );

        registry
    }
}

/// Result of effect policy evaluation
#[derive(Debug, Clone)]
pub enum EffectDecision {
    /// Apply the effect immediately
    ApplyImmediate,
    /// Create a proposal and wait for approval
    CreateProposal {
        /// Who needs to approve
        approvers: Vec<CapabilityRequirement>,
        /// Approval threshold
        threshold: ApprovalThreshold,
        /// Timeout in milliseconds
        timeout_ms: u64,
    },
    /// Run a ceremony before applying
    RunCeremony {
        /// Type of ceremony to run
        ceremony: CeremonyType,
    },
}

impl From<EffectTiming> for EffectDecision {
    fn from(timing: EffectTiming) -> Self {
        match timing {
            EffectTiming::Immediate => EffectDecision::ApplyImmediate,
            EffectTiming::Deferred {
                requires_approval_from,
                threshold,
                timeout_ms,
            } => EffectDecision::CreateProposal {
                approvers: requires_approval_from,
                threshold,
                timeout_ms,
            },
            EffectTiming::Blocking { ceremony } => EffectDecision::RunCeremony { ceremony },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_type_roundtrip() {
        let operations = vec![
            OperationType::SendMessage,
            OperationType::CreateChannel,
            OperationType::RotateGuardians,
        ];

        for op in operations {
            let s = op.as_str();
            let parsed: OperationType = s.parse().unwrap();
            assert_eq!(op, parsed);
        }
    }

    #[test]
    fn test_security_levels() {
        assert_eq!(
            OperationType::SendMessage.default_security_level(),
            SecurityLevel::Low
        );
        assert_eq!(
            OperationType::RemoveChannelMember.default_security_level(),
            SecurityLevel::Medium
        );
        assert_eq!(
            OperationType::DeleteChannel.default_security_level(),
            SecurityLevel::High
        );
        assert_eq!(
            OperationType::RotateGuardians.default_security_level(),
            SecurityLevel::Critical
        );
    }

    #[test]
    fn test_approval_threshold_any() {
        let threshold = ApprovalThreshold::Any;
        assert!(threshold.is_met(1, 5));
        assert!(!threshold.is_met(0, 5));
    }

    #[test]
    fn test_approval_threshold_unanimous() {
        let threshold = ApprovalThreshold::Unanimous;
        assert!(threshold.is_met(5, 5));
        assert!(!threshold.is_met(4, 5));
    }

    #[test]
    fn test_approval_threshold_k_of_n() {
        let threshold = ApprovalThreshold::Threshold { required: 3 };
        assert!(threshold.is_met(3, 5));
        assert!(threshold.is_met(4, 5));
        assert!(!threshold.is_met(2, 5));
    }

    #[test]
    fn test_approval_threshold_percentage() {
        let threshold = ApprovalThreshold::Percentage { percent: 50 };
        // 50% of 5 = 2.5, rounded up = 3
        assert!(threshold.is_met(3, 5));
        assert!(!threshold.is_met(2, 5));
    }

    #[test]
    fn test_registry_defaults() {
        let registry = EffectPolicyRegistry::default();

        // Low risk should be immediate
        let timing = registry.get_timing(&OperationType::SendMessage, None);
        assert!(timing.is_immediate());

        // Critical should be blocking
        let timing = registry.get_timing(&OperationType::RotateGuardians, None);
        assert!(timing.is_blocking());
    }

    #[test]
    fn test_registry_override() {
        let mut registry = EffectPolicyRegistry::default();
        let context_id = ContextId::new_from_entropy([42u8; 32]);

        // Default for RemoveChannelMember is deferred
        let timing = registry.get_timing(&OperationType::RemoveChannelMember, None);
        assert!(timing.is_deferred());

        // Override to immediate for this context
        registry.set_override(
            context_id,
            OperationType::RemoveChannelMember,
            EffectTiming::Immediate,
        );

        // With context, should use override
        let timing = registry.get_timing(&OperationType::RemoveChannelMember, Some(&context_id));
        assert!(timing.is_immediate());

        // Without context, should use default
        let timing = registry.get_timing(&OperationType::RemoveChannelMember, None);
        assert!(timing.is_deferred());
    }

    #[test]
    fn test_effect_decision_conversion() {
        let immediate = EffectTiming::Immediate;
        let decision: EffectDecision = immediate.into();
        assert!(matches!(decision, EffectDecision::ApplyImmediate));

        let blocking = EffectTiming::blocking(CeremonyType::Invitation);
        let decision: EffectDecision = blocking.into();
        assert!(matches!(
            decision,
            EffectDecision::RunCeremony {
                ceremony: CeremonyType::Invitation
            }
        ));
    }

    #[test]
    fn test_get_full_policy() {
        let registry = EffectPolicyRegistry::default();
        let policy = registry.get_policy(&OperationType::DeleteChannel, None);

        assert_eq!(policy.operation, OperationType::DeleteChannel);
        assert_eq!(policy.security_level, SecurityLevel::High);
        assert!(policy.timing.is_deferred());
    }

    #[test]
    fn test_effect_timing_helpers() {
        let timing = EffectTiming::deferred_single_admin(24);
        assert!(timing.is_deferred());

        if let EffectTiming::Deferred {
            threshold,
            timeout_ms,
            ..
        } = timing
        {
            assert!(matches!(threshold, ApprovalThreshold::Any));
            assert_eq!(timeout_ms, 24 * 60 * 60 * 1000);
        } else {
            panic!("Expected Deferred timing");
        }
    }
}
