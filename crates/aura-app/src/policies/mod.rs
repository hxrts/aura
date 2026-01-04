//! Delivery and consistency policy framework.
//!
//! This module provides policies for controlling when acknowledgment
//! tracking should be dropped and how consistency metadata is interpreted.
//!
//! # Architecture
//!
//! ```text
//! Journal Layer                    App Layer
//! ┌──────────────────┐            ┌─────────────────────┐
//! │ Fact + AckStorage│            │ DeliveryPolicy      │
//! │                  │◄───────────│  - expected_peers() │
//! │ gc_ack_tracking()│            │  - should_drop()    │
//! └──────────────────┘            └─────────────────────┘
//!                                         │
//!                                         ▼
//!                                 ┌─────────────────────┐
//!                                 │ PolicyRegistry      │
//!                                 │  - register<F>()    │
//!                                 │  - get_policy()     │
//!                                 └─────────────────────┘
//! ```

pub mod delivery;
pub mod registry;
pub mod status_interpreter;

// Re-export key types
pub use delivery::{
    boxed, BoxedPolicy, ChannelMembersPolicy, DeliveryPolicy, DropWhenFinalized,
    DropWhenFinalizedAndFullyAcked, DropWhenFullyAcked, DropWhenSafeAndFullyAcked,
    NoOpPolicyContext, PolicyContext,
};

pub use registry::{PolicyRegistry, TypedPolicyRegistry};

pub use status_interpreter::{
    CategoryKind, CeremonyDetails, NoOpStatusContext, ProposalDetails, StatusContext,
    StatusInterpreter, StatusResult,
};

// =============================================================================
// Default Policy Configuration
// =============================================================================

/// Fact type names for policy registration
pub mod fact_types {
    /// MessageSentSealed - encrypted AMP messages
    pub const MESSAGE_SENT_SEALED: &str = "MessageSentSealed";
    /// ChannelCreated - channel creation facts
    pub const CHANNEL_CREATED: &str = "ChannelCreated";
    /// InvitationAccepted - accepted invitations
    pub const INVITATION_ACCEPTED: &str = "InvitationAccepted";
    /// GuardianBinding - guardian relationship facts
    pub const GUARDIAN_BINDING: &str = "GuardianBinding";
}

/// Create a policy registry with default policies for standard fact types.
///
/// This configures:
/// - `MessageSentSealed` → `DropWhenFullyAcked` (delivery confirmation)
/// - `ChannelCreated` → `DropWhenFinalized` (consensus is enough)
/// - `InvitationAccepted` → `DropWhenFinalized` (consensus is enough)
/// - `GuardianBinding` → `DropWhenFinalizedAndFullyAcked` (critical - both needed)
///
/// # Example
///
/// ```rust,ignore
/// let registry = create_default_policy_registry();
/// let policy = registry.get_policy("MessageSentSealed");
/// assert_eq!(policy.name(), "DropWhenFullyAcked");
/// ```
pub fn create_default_policy_registry() -> PolicyRegistry {
    let mut registry = PolicyRegistry::new();

    // Messages need delivery confirmation
    registry.register(fact_types::MESSAGE_SENT_SEALED, DropWhenFullyAcked);

    // Channels just need consensus
    registry.register(fact_types::CHANNEL_CREATED, DropWhenFinalized);

    // Invitations just need consensus
    registry.register(fact_types::INVITATION_ACCEPTED, DropWhenFinalized);

    // Guardian bindings are critical - need both
    registry.register(
        fact_types::GUARDIAN_BINDING,
        DropWhenFinalizedAndFullyAcked,
    );

    registry
}
