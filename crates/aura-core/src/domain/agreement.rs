//! Agreement Types for the A1/A2/A3 Taxonomy
//!
//! This module implements the agreement level taxonomy from `docs/107_operation_categories.md`.
//! Agreement indicates how durable/finalized a fact is:
//!
//! - **A1 (Provisional)**: Usable immediately, may be superseded
//! - **A2 (SoftSafe)**: Bounded divergence with convergence certificate
//! - **A3 (Finalized)**: Consensus-confirmed, durable, non-forkable
//!
//! This is distinct from the `Finality` enum in `temporal.rs` which tracks
//! replication and durability stages. `Agreement` captures the distributed
//! systems agreement semantics.

use crate::query::ConsensusId;
use crate::time::PhysicalTime;
use crate::Hash32;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Convergence Certificate
// ─────────────────────────────────────────────────────────────────────────────

/// A certificate proving bounded divergence with expected convergence.
///
/// Used for A2 (SoftSafe) agreement level. The certificate provides:
/// - Proof of coordinator acknowledgment
/// - Bounded divergence window
/// - Expected convergence time
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceCert {
    /// Hash of the coordinator's acknowledgment
    pub coordinator_ack: Hash32,

    /// Maximum divergence bound (number of operations)
    pub divergence_bound: u32,

    /// Expected convergence time
    pub expected_convergence: PhysicalTime,

    /// Coordinator authority that issued this certificate
    pub coordinator_id: Option<String>,
}

impl ConvergenceCert {
    /// Create a new convergence certificate
    pub fn new(
        coordinator_ack: Hash32,
        divergence_bound: u32,
        expected_convergence: PhysicalTime,
    ) -> Self {
        Self {
            coordinator_ack,
            divergence_bound,
            expected_convergence,
            coordinator_id: None,
        }
    }

    /// Set the coordinator ID
    #[must_use]
    pub fn with_coordinator(mut self, coordinator_id: impl Into<String>) -> Self {
        self.coordinator_id = Some(coordinator_id.into());
        self
    }

    /// Check if this certificate has expired based on the expected convergence time
    pub fn is_expired(&self, now: PhysicalTime) -> bool {
        now > self.expected_convergence
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Agreement Level
// ─────────────────────────────────────────────────────────────────────────────

/// Agreement level for facts, mapping to the A1/A2/A3 taxonomy.
///
/// This enum represents the distributed systems agreement semantics:
///
/// - **Provisional (A1)**: The fact is usable immediately but may be superseded.
///   Fast paths use this for immediate usability before consensus completes.
///
/// - **SoftSafe (A2)**: Bounded divergence with a convergence certificate.
///   Provides stronger guarantees than A1 but weaker than A3. The certificate
///   proves that divergence is bounded and convergence is expected.
///
/// - **Finalized (A3)**: Consensus-confirmed, durable, and non-forkable.
///   This is the strongest agreement level and is required for durable shared state.
///
/// # Relationship to Finality
///
/// `Agreement` is orthogonal to `Finality` (in `temporal.rs`):
/// - `Finality` tracks replication/durability (Local → Replicated → Checkpointed → Consensus)
/// - `Agreement` tracks distributed agreement semantics (Provisional → SoftSafe → Finalized)
///
/// A fact can be `Finality::Local` but `Agreement::Finalized` (consensus completed
/// but not yet replicated), or `Finality::Replicated` but `Agreement::Provisional`
/// (replicated optimistically before consensus).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Agreement {
    /// A1: Provisional - usable immediately, may be superseded.
    ///
    /// Used for optimistic operations that apply immediately but haven't
    /// been confirmed by consensus. These may be rolled back if superseded.
    #[default]
    Provisional,

    /// A2: Soft-Safe - bounded divergence with convergence certificate.
    ///
    /// Provides coordinator-mediated safety with bounded divergence.
    /// Stronger than A1 but doesn't require full consensus.
    SoftSafe {
        /// Optional convergence certificate proving bounded divergence
        cert: Option<ConvergenceCert>,
    },

    /// A3: Finalized - consensus-confirmed, durable, non-forkable.
    ///
    /// The strongest agreement level. Once finalized, the fact cannot be
    /// superseded or rolled back. Required for durable shared state.
    Finalized {
        /// The consensus instance that confirmed this fact
        consensus_id: ConsensusId,
    },
}

impl Agreement {
    /// Create a provisional agreement (A1)
    pub fn provisional() -> Self {
        Self::Provisional
    }

    /// Create a soft-safe agreement (A2) without a certificate
    pub fn soft_safe() -> Self {
        Self::SoftSafe { cert: None }
    }

    /// Create a soft-safe agreement (A2) with a certificate
    pub fn soft_safe_with_cert(cert: ConvergenceCert) -> Self {
        Self::SoftSafe { cert: Some(cert) }
    }

    /// Create a finalized agreement (A3)
    pub fn finalized(consensus_id: ConsensusId) -> Self {
        Self::Finalized { consensus_id }
    }

    /// Simple boolean for hot-path UI ("is this permanent?")
    ///
    /// Returns true only for A3 (Finalized) agreement.
    pub fn is_finalized(&self) -> bool {
        matches!(self, Self::Finalized { .. })
    }

    /// Is this at least soft-safe (A2 or A3)?
    ///
    /// Returns true for both SoftSafe and Finalized.
    pub fn is_safe(&self) -> bool {
        matches!(self, Self::SoftSafe { .. } | Self::Finalized { .. })
    }

    /// Is this provisional (A1)?
    pub fn is_provisional(&self) -> bool {
        matches!(self, Self::Provisional)
    }

    /// Get the consensus ID if finalized
    pub fn consensus_id(&self) -> Option<ConsensusId> {
        match self {
            Self::Finalized { consensus_id } => Some(*consensus_id),
            _ => None,
        }
    }

    /// Get the convergence certificate if soft-safe
    pub fn convergence_cert(&self) -> Option<&ConvergenceCert> {
        match self {
            Self::SoftSafe { cert } => cert.as_ref(),
            _ => None,
        }
    }

    /// Get the agreement level as a string for display
    pub fn level_str(&self) -> &'static str {
        match self {
            Self::Provisional => "A1:Provisional",
            Self::SoftSafe { .. } => "A2:SoftSafe",
            Self::Finalized { .. } => "A3:Finalized",
        }
    }

    /// Get a numeric strength level for comparison
    ///
    /// - A1 (Provisional) = 1
    /// - A2 (SoftSafe) = 2
    /// - A3 (Finalized) = 3
    pub fn strength(&self) -> u8 {
        match self {
            Self::Provisional => 1,
            Self::SoftSafe { .. } => 2,
            Self::Finalized { .. } => 3,
        }
    }

    /// Check if this agreement is at least as strong as another
    pub fn is_at_least(&self, other: &Agreement) -> bool {
        self.strength() >= other.strength()
    }
}

impl std::fmt::Display for Agreement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provisional => write!(f, "Provisional"),
            Self::SoftSafe { cert: None } => write!(f, "SoftSafe"),
            Self::SoftSafe { cert: Some(_) } => write!(f, "SoftSafe(certified)"),
            Self::Finalized { consensus_id } => {
                write!(f, "Finalized({:?})", &consensus_id.0[..4])
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_time(millis: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms: millis,
            uncertainty: None,
        }
    }

    #[test]
    fn test_agreement_provisional() {
        let agreement = Agreement::provisional();
        assert!(agreement.is_provisional());
        assert!(!agreement.is_safe());
        assert!(!agreement.is_finalized());
        assert_eq!(agreement.strength(), 1);
    }

    #[test]
    fn test_agreement_soft_safe() {
        let agreement = Agreement::soft_safe();
        assert!(!agreement.is_provisional());
        assert!(agreement.is_safe());
        assert!(!agreement.is_finalized());
        assert_eq!(agreement.strength(), 2);
        assert!(agreement.convergence_cert().is_none());
    }

    #[test]
    fn test_agreement_soft_safe_with_cert() {
        let cert = ConvergenceCert::new(Hash32([0; 32]), 100, test_time(1000));
        let agreement = Agreement::soft_safe_with_cert(cert);
        assert!(agreement.is_safe());
        assert!(agreement.convergence_cert().is_some());
    }

    #[test]
    fn test_agreement_finalized() {
        let consensus_id = ConsensusId::new([1; 32]);
        let agreement = Agreement::finalized(consensus_id);
        assert!(!agreement.is_provisional());
        assert!(agreement.is_safe());
        assert!(agreement.is_finalized());
        assert_eq!(agreement.strength(), 3);
        assert_eq!(agreement.consensus_id(), Some(consensus_id));
    }

    #[test]
    fn test_agreement_ordering() {
        let provisional = Agreement::provisional();
        let soft_safe = Agreement::soft_safe();
        let finalized = Agreement::finalized(ConsensusId::new([0; 32]));

        assert!(soft_safe.is_at_least(&provisional));
        assert!(finalized.is_at_least(&soft_safe));
        assert!(finalized.is_at_least(&provisional));
        assert!(!provisional.is_at_least(&soft_safe));
    }

    #[test]
    fn test_agreement_display() {
        assert_eq!(Agreement::provisional().to_string(), "Provisional");
        assert_eq!(Agreement::soft_safe().to_string(), "SoftSafe");

        let cert = ConvergenceCert::new(Hash32([0; 32]), 100, test_time(1000));
        assert_eq!(
            Agreement::soft_safe_with_cert(cert).to_string(),
            "SoftSafe(certified)"
        );
    }

    #[test]
    fn test_convergence_cert() {
        let cert = ConvergenceCert::new(Hash32([1; 32]), 50, test_time(5000))
            .with_coordinator("coordinator-1");

        assert_eq!(cert.divergence_bound, 50);
        assert_eq!(cert.coordinator_id, Some("coordinator-1".to_string()));

        // Check expiration
        assert!(!cert.is_expired(test_time(4000)));
        assert!(cert.is_expired(test_time(6000)));
    }

    #[test]
    fn test_agreement_level_str() {
        assert_eq!(Agreement::provisional().level_str(), "A1:Provisional");
        assert_eq!(Agreement::soft_safe().level_str(), "A2:SoftSafe");
        assert_eq!(
            Agreement::finalized(ConsensusId::new([0; 32])).level_str(),
            "A3:Finalized"
        );
    }
}
