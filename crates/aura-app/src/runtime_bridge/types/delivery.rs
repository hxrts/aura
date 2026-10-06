//! Outbound chat delivery failure DTOs reported to the runtime diagnostics log.

use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};

/// Why a committed chat message could not be delivered to a remote peer.
/// Observation only: the message's own delivery status carries the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundDeliveryFailureCause {
    /// No authoritative recipient peer resolved within the retry budget.
    NoRecipients,
    /// Peer channel or connectivity prerequisites never converged.
    PrerequisitesNeverConverged { detail: String },
    /// The recipient could not be reached within the retry budget.
    RecipientUnreachable { detail: String },
    /// Delivery failed for another reason.
    Other { detail: String },
}

impl std::fmt::Display for OutboundDeliveryFailureCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRecipients => write!(f, "no_recipients"),
            Self::PrerequisitesNeverConverged { detail } => {
                write!(f, "prerequisites_never_converged: {detail}")
            }
            Self::RecipientUnreachable { detail } => write!(f, "recipient_unreachable: {detail}"),
            Self::Other { detail } => write!(f, "delivery_failed: {detail}"),
        }
    }
}

/// One outbound chat message delivery failure. `recipient` is `None` when
/// the failure is not attributable to a single peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessageDeliveryFailure {
    pub context_id: ContextId,
    pub channel_id: ChannelId,
    pub message_id: String,
    pub recipient: Option<AuthorityId>,
    pub cause: OutboundDeliveryFailureCause,
}
