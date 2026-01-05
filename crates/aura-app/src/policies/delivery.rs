//! Delivery policy framework for ack tracking lifecycle.
//!
//! This module provides the `DeliveryPolicy` trait and standard implementations
//! for controlling when acknowledgment tracking should be dropped for facts.
//!
//! # Overview
//!
//! Different fact types have different delivery requirements:
//! - Messages may need to know when all recipients received them
//! - Invitations may only need to know they reached consensus
//! - Critical operations may need both finalization AND acknowledgment
//!
//! # Standard Policies
//!
//! | Policy | Drops ack tracking when... | Use case |
//! |--------|---------------------------|----------|
//! | `DropWhenFinalized` | Fact reaches A3 (consensus) | Non-critical, just needs durability |
//! | `DropWhenFullyAcked` | All expected peers acked | Messages, needs delivery confirmation |
//! | `DropWhenFinalizedAndFullyAcked` | Both finalized AND fully acked | Critical operations |
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_app::policies::{DeliveryPolicy, DropWhenFullyAcked};
//!
//! // Get expected peers from context
//! let policy = DropWhenFullyAcked;
//! let expected = policy.expected_peers(&fact, &context);
//!
//! // Check if we should stop tracking
//! if policy.should_drop_tracking(&consistency, &expected) {
//!     journal.delete_acks(&fact_id);
//! }
//! ```

use aura_core::domain::{Acknowledgment, Consistency};
use aura_core::identifiers::AuthorityId;
use aura_journal::Fact;
use std::sync::Arc;

// =============================================================================
// DeliveryPolicy Trait
// =============================================================================

/// Policy for controlling acknowledgment tracking lifecycle.
///
/// The app layer implements this to define what "delivery complete" means
/// for each fact type. The journal layer uses these policies during GC.
pub trait DeliveryPolicy: Send + Sync {
    /// Get the peers expected to acknowledge this fact.
    ///
    /// This determines who should receive the fact for it to be considered
    /// "delivered". Context is passed to allow looking up channel members, etc.
    fn expected_peers(&self, fact: &Fact, context: &dyn PolicyContext) -> Vec<AuthorityId>;

    /// Check if ack tracking should be dropped for this fact.
    ///
    /// When this returns true, the journal will:
    /// 1. Delete ack records for this fact
    /// 2. Set `ack_tracked = false` on the fact
    fn should_drop_tracking(
        &self,
        consistency: &Consistency,
        expected: &[AuthorityId],
    ) -> bool;

    /// Get a human-readable name for this policy
    fn name(&self) -> &'static str;
}

/// Context for policy evaluation.
///
/// Provides access to app-level state needed for policy decisions.
pub trait PolicyContext: Send + Sync {
    /// Get the members of a channel (for message delivery)
    fn channel_members(&self, channel_id: &str) -> Vec<AuthorityId>;

    /// Get the guardians for an authority (for critical operations)
    fn guardians(&self, authority_id: &AuthorityId) -> Vec<AuthorityId>;

    /// Get all known peers in a context
    fn context_peers(&self, context_id: &str) -> Vec<AuthorityId>;
}

// =============================================================================
// Standard Policy Implementations
// =============================================================================

/// Drop ack tracking once the fact is finalized (A3 consensus).
///
/// Use this for facts where consensus durability is sufficient,
/// and you don't need to know about individual peer delivery.
#[derive(Debug, Clone, Copy, Default)]
pub struct DropWhenFinalized;

impl DeliveryPolicy for DropWhenFinalized {
    fn expected_peers(&self, _fact: &Fact, _ctx: &dyn PolicyContext) -> Vec<AuthorityId> {
        // For finalized-only policy, we don't track individual peers
        Vec::new()
    }

    fn should_drop_tracking(&self, c: &Consistency, _expected: &[AuthorityId]) -> bool {
        c.agreement.is_finalized()
    }

    fn name(&self) -> &'static str {
        "DropWhenFinalized"
    }
}

/// Drop ack tracking once all expected peers have acknowledged.
///
/// Use this for messages and other facts where you need to know
/// that specific peers received the content.
#[derive(Debug, Clone, Copy, Default)]
pub struct DropWhenFullyAcked;

impl DropWhenFullyAcked {
    /// Check if all expected peers have acknowledged
    fn is_fully_acked(ack: Option<&Acknowledgment>, expected: &[AuthorityId]) -> bool {
        let Some(ack) = ack else {
            return expected.is_empty();
        };
        expected.iter().all(|p| ack.contains(p))
    }
}

impl DeliveryPolicy for DropWhenFullyAcked {
    fn expected_peers(&self, _fact: &Fact, _ctx: &dyn PolicyContext) -> Vec<AuthorityId> {
        // Subclasses or wrapper policies should override to get actual members
        // This default returns empty - caller should provide expected peers
        Vec::new()
    }

    fn should_drop_tracking(&self, c: &Consistency, expected: &[AuthorityId]) -> bool {
        Self::is_fully_acked(c.acknowledgment.as_ref(), expected)
    }

    fn name(&self) -> &'static str {
        "DropWhenFullyAcked"
    }
}

/// Drop ack tracking only when BOTH finalized AND fully acknowledged.
///
/// Use this for critical operations where you need:
/// 1. Consensus confirmation (durability)
/// 2. Peer confirmation (delivery)
#[derive(Debug, Clone, Copy, Default)]
pub struct DropWhenFinalizedAndFullyAcked;

impl DeliveryPolicy for DropWhenFinalizedAndFullyAcked {
    fn expected_peers(&self, _fact: &Fact, _ctx: &dyn PolicyContext) -> Vec<AuthorityId> {
        Vec::new()
    }

    fn should_drop_tracking(&self, c: &Consistency, expected: &[AuthorityId]) -> bool {
        let is_finalized = c.agreement.is_finalized();
        let is_fully_acked = DropWhenFullyAcked::is_fully_acked(c.acknowledgment.as_ref(), expected);
        is_finalized && is_fully_acked
    }

    fn name(&self) -> &'static str {
        "DropWhenFinalizedAndFullyAcked"
    }
}

/// Policy that checks if consensus is safe (A2 or A3) and fully acked.
///
/// Use for operations that can proceed once soft-safe and delivered.
#[derive(Debug, Clone, Copy, Default)]
pub struct DropWhenSafeAndFullyAcked;

impl DeliveryPolicy for DropWhenSafeAndFullyAcked {
    fn expected_peers(&self, _fact: &Fact, _ctx: &dyn PolicyContext) -> Vec<AuthorityId> {
        Vec::new()
    }

    fn should_drop_tracking(&self, c: &Consistency, expected: &[AuthorityId]) -> bool {
        let is_safe = c.agreement.is_safe();
        let is_fully_acked = DropWhenFullyAcked::is_fully_acked(c.acknowledgment.as_ref(), expected);
        is_safe && is_fully_acked
    }

    fn name(&self) -> &'static str {
        "DropWhenSafeAndFullyAcked"
    }
}

// =============================================================================
// Dynamic Policy Wrapper
// =============================================================================

/// A boxed policy for dynamic dispatch.
pub type BoxedPolicy = Arc<dyn DeliveryPolicy>;

/// Create a boxed policy from a static policy
pub fn boxed<P: DeliveryPolicy + 'static>(policy: P) -> BoxedPolicy {
    Arc::new(policy)
}

// =============================================================================
// Channel-Aware Policies
// =============================================================================

/// Type alias for channel ID extractor functions
pub type ChannelIdExtractor = Box<dyn Fn(&Fact) -> Option<String> + Send + Sync>;

/// Policy that delivers to all channel members.
///
/// This wraps another policy and provides channel member lookup.
pub struct ChannelMembersPolicy<P: DeliveryPolicy> {
    inner: P,
    channel_id_extractor: ChannelIdExtractor,
}

impl<P: DeliveryPolicy> ChannelMembersPolicy<P> {
    /// Create a new channel members policy
    pub fn new(
        inner: P,
        channel_id_extractor: impl Fn(&Fact) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner,
            channel_id_extractor: Box::new(channel_id_extractor),
        }
    }
}

impl<P: DeliveryPolicy> DeliveryPolicy for ChannelMembersPolicy<P> {
    fn expected_peers(&self, fact: &Fact, ctx: &dyn PolicyContext) -> Vec<AuthorityId> {
        if let Some(channel_id) = (self.channel_id_extractor)(fact) {
            ctx.channel_members(&channel_id)
        } else {
            Vec::new()
        }
    }

    fn should_drop_tracking(&self, c: &Consistency, expected: &[AuthorityId]) -> bool {
        self.inner.should_drop_tracking(c, expected)
    }

    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

// =============================================================================
// No-Op Context for Testing
// =============================================================================

/// A no-op policy context for testing
#[derive(Debug, Clone, Default)]
pub struct NoOpPolicyContext;

impl PolicyContext for NoOpPolicyContext {
    fn channel_members(&self, _channel_id: &str) -> Vec<AuthorityId> {
        Vec::new()
    }

    fn guardians(&self, _authority_id: &AuthorityId) -> Vec<AuthorityId> {
        Vec::new()
    }

    fn context_peers(&self, _context_id: &str) -> Vec<AuthorityId> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::domain::{Acknowledgment, Agreement, OperationCategory, Propagation};

    fn test_consistency(agreement: Agreement, ack: Option<Acknowledgment>) -> Consistency {
        Consistency {
            category: OperationCategory::Optimistic,
            agreement,
            propagation: Propagation::Complete,
            acknowledgment: ack,
        }
    }

    fn test_authority(n: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([n; 32])
    }

    fn test_ack(peers: &[u8]) -> Acknowledgment {
        let mut ack = Acknowledgment::default();
        for &n in peers {
            ack.acked_by.push(aura_core::domain::AckRecord {
                peer: test_authority(n),
                acked_at: aura_core::time::PhysicalTime {
                    ts_ms: 1000,
                    uncertainty: None,
                },
            });
        }
        ack
    }

    #[test]
    fn test_drop_when_finalized() {
        let policy = DropWhenFinalized;

        // Provisional - don't drop
        let c = test_consistency(Agreement::Provisional, None);
        assert!(!policy.should_drop_tracking(&c, &[]));

        // Finalized - drop
        let c = test_consistency(
            Agreement::Finalized {
                consensus_id: aura_core::query::ConsensusId([1u8; 32]),
            },
            None,
        );
        assert!(policy.should_drop_tracking(&c, &[]));
    }

    #[test]
    fn test_drop_when_fully_acked() {
        let policy = DropWhenFullyAcked;
        let expected = vec![test_authority(1), test_authority(2)];

        // No acks - don't drop
        let c = test_consistency(Agreement::Provisional, None);
        assert!(!policy.should_drop_tracking(&c, &expected));

        // Partial acks - don't drop
        let c = test_consistency(Agreement::Provisional, Some(test_ack(&[1])));
        assert!(!policy.should_drop_tracking(&c, &expected));

        // All acked - drop
        let c = test_consistency(Agreement::Provisional, Some(test_ack(&[1, 2])));
        assert!(policy.should_drop_tracking(&c, &expected));

        // Empty expected means always acked
        assert!(policy.should_drop_tracking(&c, &[]));
    }

    #[test]
    fn test_drop_when_finalized_and_fully_acked() {
        let policy = DropWhenFinalizedAndFullyAcked;
        let expected = vec![test_authority(1)];

        // Finalized but not acked - don't drop
        let c = test_consistency(
            Agreement::Finalized {
                consensus_id: aura_core::query::ConsensusId([1u8; 32]),
            },
            None,
        );
        assert!(!policy.should_drop_tracking(&c, &expected));

        // Acked but not finalized - don't drop
        let c = test_consistency(Agreement::Provisional, Some(test_ack(&[1])));
        assert!(!policy.should_drop_tracking(&c, &expected));

        // Both finalized AND acked - drop
        let c = test_consistency(
            Agreement::Finalized {
                consensus_id: aura_core::query::ConsensusId([1u8; 32]),
            },
            Some(test_ack(&[1])),
        );
        assert!(policy.should_drop_tracking(&c, &expected));
    }
}
