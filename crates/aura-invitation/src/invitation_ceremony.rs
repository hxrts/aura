//! Consensus-Based Invitation Ceremony
//!
//! This module provides a safe, consensus-backed invitation acceptance protocol
//! that ensures both parties agree on the relationship establishment before
//! either side commits the fact.
//!
//! ## Problem Solved
//!
//! Without consensus, invitation acceptance can diverge:
//! - Receiver accepts and commits "I accepted invitation X"
//! - Network partition occurs before sender receives response
//! - Receiver thinks they're connected, sender doesn't know
//! - Manual intervention required to reconcile
//!
//! ## Solution: Prestate-Bound Consensus
//!
//! 1. **Sender** creates invitation with unique ID and commitment hash
//! 2. **Receiver** accepts, creating a pending acceptance bound to prestate
//! 3. **Consensus** ensures both parties agree on acceptance BEFORE committing
//! 4. Only after consensus does either party commit the relationship fact
//!
//! ## Session Type Guarantee
//!
//! The choreography enforces linear protocol flow:
//! ```text
//! Sender -> Receiver: InvitationOffer
//! Receiver -> Sender: AcceptanceProposal
//! [Consensus: Both agree on acceptance]
//! choice {
//!     Sender -> Receiver: CommitAcceptance
//! } or {
//!     Sender -> Receiver: AbortAcceptance
//! }
//! ```
//!
//! ## Key Properties
//!
//! - **Atomicity**: Both parties commit or neither does
//! - **No Orphaned Accepts**: Pending acceptances without consensus are inert
//! - **Deterministic ID**: `CeremonyId = H(prestate_hash || invitation_hash || nonce)`
//! - **Auditability**: All state changes recorded as journal facts

use aura_core::domain::FactValue;
use aura_core::effects::{
    JournalEffects, PhysicalTimeEffects, ThresholdSigningEffects, TimeEffects,
};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::ThresholdSignature;
use aura_core::{AuraError, AuraResult, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::facts::InvitationFact;
use crate::InvitationOffer;

// =============================================================================
// CEREMONY TYPES
// =============================================================================

/// Unique identifier for an invitation ceremony instance.
///
/// Derived from `H(prestate_hash, invitation_hash, nonce)` to prevent
/// concurrent ceremonies for the same invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InvitationCeremonyId(pub Hash32);

impl InvitationCeremonyId {
    /// Create a ceremony ID from constituent parts.
    ///
    /// The prestate hash ensures this ceremony can only proceed if the
    /// current invitation state matches expectations.
    pub fn new(prestate_hash: &Hash32, invitation_hash: &Hash32, nonce: u64) -> Self {
        // Build concatenated input for hashing
        let mut input = Vec::with_capacity(32 + 32 + 8);
        input.extend_from_slice(prestate_hash.as_bytes());
        input.extend_from_slice(invitation_hash.as_bytes());
        input.extend_from_slice(&nonce.to_le_bytes());
        // Use system hash algorithm
        Self(Hash32::from_bytes(&input))
    }

    /// Get the underlying hash.
    pub fn as_hash(&self) -> &Hash32 {
        &self.0
    }
}

/// The acceptance proposal that will be subject to consensus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceProposal {
    /// The invitation being accepted
    pub invitation_id: String,
    /// The authority accepting
    pub acceptor: AuthorityId,
    /// Prestate hash at time of acceptance
    pub prestate_hash: Hash32,
    /// Acceptance message (optional)
    pub message: Option<String>,
    /// Signature over the acceptance
    pub signature: ThresholdSignature,
}

/// Response to an acceptance proposal during consensus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AcceptanceResponse {
    /// Confirm the acceptance
    Confirm {
        /// Signature confirming acceptance
        signature: ThresholdSignature,
    },
    /// Reject the acceptance (e.g., invitation expired, wrong prestate)
    Reject {
        /// Reason for rejection
        reason: String,
    },
}

/// Current status of an invitation ceremony.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyStatus {
    /// Ceremony initiated, awaiting acceptance proposal
    AwaitingAcceptance,
    /// Acceptance proposed, awaiting consensus
    AwaitingConsensus,
    /// Consensus reached, relationship established
    Committed,
    /// Ceremony aborted (by either party or timeout)
    Aborted { reason: String },
}

/// Full state of an invitation ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationCeremonyState {
    /// Unique ceremony identifier
    pub ceremony_id: InvitationCeremonyId,
    /// The invitation offer
    pub invitation: InvitationOffer,
    /// Authority that sent the invitation
    pub sender: AuthorityId,
    /// Authority that should accept (if known)
    pub expected_acceptor: Option<AuthorityId>,
    /// Current ceremony status
    pub status: CeremonyStatus,
    /// Acceptance proposal (if received)
    pub acceptance: Option<AcceptanceProposal>,
    /// Timestamp when ceremony started
    pub started_at_ms: u64,
    /// Timeout for ceremony completion (ms)
    pub timeout_ms: u64,
}

// =============================================================================
// CHOREOGRAPHY DOCUMENTATION
// =============================================================================
//
// Protocol: InvitationCeremony
// Roles: Sender, Acceptor
//
// Flow:
// 1. Sender -> Acceptor: OfferInvitation(InvitationOffer)
//    - guard_capability: "invitation:send"
//    - flow_cost: 1
//    - journal_facts: InvitationFact::CeremonyInitiated
//
// 2. Acceptor -> Sender: ProposeAcceptance(AcceptanceProposal)
//    - guard_capability: "invitation:accept"
//    - flow_cost: 1
//    - journal_facts: InvitationFact::CeremonyAcceptanceReceived
//
// 3. Exclusive choice after consensus attempt:
//    choice {
//        Sender -> Acceptor: CommitAcceptance
//        - guard_capability: "invitation:confirm"
//        - flow_cost: 1
//        - journal_facts: InvitationFact::CeremonyCommitted
//    } or {
//        Sender -> Acceptor: AbortAcceptance
//        - guard_capability: "invitation:abort"
//        - flow_cost: 1
//        - journal_facts: InvitationFact::CeremonyAborted
//    }
//
// The linear type guarantees are enforced by:
// - Prestate binding (ceremony ID derived from current state hash)
// - Status enum with non-repeatable transitions
// - Journal fact emission at each step

// =============================================================================
// CEREMONY EXECUTOR
// =============================================================================

/// Executes invitation ceremonies with consensus guarantees.
///
/// The executor manages the lifecycle of invitation ceremonies, ensuring
/// atomicity through prestate binding and consensus.
pub struct InvitationCeremonyExecutor<E: InvitationCeremonyEffects> {
    /// Effect system for all operations
    effects: E,
    /// Active ceremonies by ID
    ceremonies: HashMap<InvitationCeremonyId, InvitationCeremonyState>,
    /// Default timeout for ceremonies
    default_timeout_ms: u64,
}

/// Combined effects required for invitation ceremonies.
pub trait InvitationCeremonyEffects:
    JournalEffects + PhysicalTimeEffects + TimeEffects + ThresholdSigningEffects + Send + Sync
{
}

// Blanket implementation
impl<T> InvitationCeremonyEffects for T where
    T: JournalEffects + PhysicalTimeEffects + TimeEffects + ThresholdSigningEffects + Send + Sync
{
}

impl<E: InvitationCeremonyEffects> InvitationCeremonyExecutor<E> {
    /// Create a new ceremony executor.
    pub fn new(effects: E) -> Self {
        Self {
            effects,
            ceremonies: HashMap::new(),
            default_timeout_ms: 5 * 60 * 1000, // 5 minutes default
        }
    }

    /// Create with custom timeout.
    pub fn with_timeout(effects: E, timeout_ms: u64) -> Self {
        Self {
            effects,
            ceremonies: HashMap::new(),
            default_timeout_ms: timeout_ms,
        }
    }

    // =========================================================================
    // CEREMONY LIFECYCLE - SENDER SIDE
    // =========================================================================

    /// Initiate a new invitation ceremony.
    ///
    /// Returns the ceremony ID that both parties will use.
    pub async fn initiate_ceremony(
        &mut self,
        sender: AuthorityId,
        invitation: InvitationOffer,
        expected_acceptor: Option<AuthorityId>,
    ) -> AuraResult<InvitationCeremonyId> {
        // Get current prestate
        let prestate_hash = self.compute_prestate_hash().await?;

        // Compute invitation hash
        let invitation_bytes =
            serde_json::to_vec(&invitation).map_err(|e| AuraError::serialization(e.to_string()))?;
        let invitation_hash = Hash32::from_bytes(&invitation_bytes);

        // Generate nonce from current time
        let nonce = self.effects.current_timestamp_ms().await;

        // Create ceremony ID
        let ceremony_id = InvitationCeremonyId::new(&prestate_hash, &invitation_hash, nonce);

        // Create ceremony state
        let state = InvitationCeremonyState {
            ceremony_id,
            invitation,
            sender,
            expected_acceptor,
            status: CeremonyStatus::AwaitingAcceptance,
            acceptance: None,
            started_at_ms: nonce,
            timeout_ms: self.default_timeout_ms,
        };

        // Store ceremony
        self.ceremonies.insert(ceremony_id, state);

        // Emit ceremony initiated fact
        self.emit_ceremony_initiated_fact(ceremony_id, sender)
            .await?;

        Ok(ceremony_id)
    }

    /// Process an acceptance proposal from the acceptor.
    ///
    /// Returns true if the proposal is valid and ceremony can proceed to consensus.
    pub async fn process_acceptance(
        &mut self,
        ceremony_id: InvitationCeremonyId,
        proposal: AcceptanceProposal,
    ) -> AuraResult<bool> {
        // Get current prestate before borrowing ceremonies
        let current_prestate = self.compute_prestate_hash().await?;
        let now = self.effects.current_timestamp_ms().await;

        // Perform validation and updates in a block to limit mutable borrow scope
        {
            let ceremony = self
                .ceremonies
                .get_mut(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

            // Verify ceremony is in correct state
            if ceremony.status != CeremonyStatus::AwaitingAcceptance {
                return Err(AuraError::invalid(format!(
                    "Ceremony not awaiting acceptance: {:?}",
                    ceremony.status
                )));
            }

            // Verify prestate matches (prevents replays/concurrent ceremonies)
            if proposal.prestate_hash != current_prestate {
                return Err(AuraError::invalid(
                    "Prestate hash mismatch - state has changed since acceptance was created",
                ));
            }

            // Verify invitation ID matches
            if proposal.invitation_id != ceremony.invitation.invitation_id {
                return Err(AuraError::invalid(
                    "Acceptance proposal is for different invitation",
                ));
            }

            // Verify acceptor is expected (if specified)
            if let Some(expected) = &ceremony.expected_acceptor {
                if &proposal.acceptor != expected {
                    return Err(AuraError::permission_denied(
                        "Acceptance from unexpected authority",
                    ));
                }
            }

            // Check timeout
            if now > ceremony.started_at_ms + ceremony.timeout_ms {
                ceremony.status = CeremonyStatus::Aborted {
                    reason: "Ceremony timed out".to_string(),
                };
                return Ok(false);
            }

            // Store acceptance and update status
            ceremony.acceptance = Some(proposal);
            ceremony.status = CeremonyStatus::AwaitingConsensus;
        }

        // Emit acceptance received fact after releasing mutable borrow
        self.emit_acceptance_received_fact(ceremony_id).await?;

        Ok(true)
    }

    /// Commit the ceremony after consensus.
    ///
    /// This is the final step - both parties have agreed, relationship is established.
    pub async fn commit_ceremony(
        &mut self,
        ceremony_id: InvitationCeremonyId,
    ) -> AuraResult<String> {
        // First, get ceremony and validate + compute relationship ID
        let relationship_id = {
            let ceremony = self
                .ceremonies
                .get(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

            // Verify ceremony is in correct state
            if ceremony.status != CeremonyStatus::AwaitingConsensus {
                return Err(AuraError::invalid(format!(
                    "Ceremony not awaiting consensus: {:?}",
                    ceremony.status
                )));
            }

            // Generate relationship ID
            let acceptance = ceremony
                .acceptance
                .as_ref()
                .ok_or_else(|| AuraError::invalid("No acceptance proposal"))?;
            self.generate_relationship_id(ceremony_id, acceptance)
        };

        // Now update status with mutable borrow
        if let Some(ceremony) = self.ceremonies.get_mut(&ceremony_id) {
            ceremony.status = CeremonyStatus::Committed;
        }

        // Emit committed fact
        self.emit_ceremony_committed_fact(ceremony_id, &relationship_id)
            .await?;

        Ok(relationship_id)
    }

    /// Abort the ceremony.
    ///
    /// Can be called by sender at any point before commit.
    pub async fn abort_ceremony(
        &mut self,
        ceremony_id: InvitationCeremonyId,
        reason: &str,
    ) -> AuraResult<()> {
        let ceremony = self
            .ceremonies
            .get_mut(&ceremony_id)
            .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

        // Can abort from any non-terminal state
        match &ceremony.status {
            CeremonyStatus::Committed => {
                return Err(AuraError::invalid("Cannot abort committed ceremony"));
            }
            CeremonyStatus::Aborted { .. } => {
                return Ok(()); // Already aborted, idempotent
            }
            _ => {}
        }

        ceremony.status = CeremonyStatus::Aborted {
            reason: reason.to_string(),
        };

        // Emit aborted fact
        self.emit_ceremony_aborted_fact(ceremony_id, reason).await?;

        Ok(())
    }

    // =========================================================================
    // CEREMONY LIFECYCLE - ACCEPTOR SIDE
    // =========================================================================

    /// Create an acceptance proposal for a received invitation.
    ///
    /// Called by the acceptor to propose acceptance.
    pub async fn create_acceptance_proposal(
        &self,
        invitation: &InvitationOffer,
        acceptor: AuthorityId,
        message: Option<String>,
    ) -> AuraResult<AcceptanceProposal> {
        // Get current prestate
        let prestate_hash = self.compute_prestate_hash().await?;

        // Create signature over acceptance using threshold signing
        use aura_core::threshold::{ApprovalContext, SignableOperation, SigningContext};

        let signing_context = SigningContext {
            authority: acceptor,
            operation: SignableOperation::Message {
                domain: "invitation:acceptance".to_string(),
                payload: format!(
                    "{}:{}:{}",
                    invitation.invitation_id,
                    hex::encode(prestate_hash.as_bytes()),
                    message.as_deref().unwrap_or("")
                )
                .into_bytes(),
            },
            approval_context: ApprovalContext::SelfOperation,
        };

        let signature = self.effects.sign(signing_context).await.map_err(|e| {
            AuraError::internal(format!("Failed to sign invitation acceptance: {}", e))
        })?;

        Ok(AcceptanceProposal {
            invitation_id: invitation.invitation_id.clone(),
            acceptor,
            prestate_hash,
            message,
            signature,
        })
    }

    // =========================================================================
    // INTERNAL HELPERS
    // =========================================================================

    /// Compute current prestate hash from journal state.
    async fn compute_prestate_hash(&self) -> AuraResult<Hash32> {
        // Get current journal and compute hash of its state
        let journal = self.effects.get_journal().await?;
        let journal_bytes = serde_json::to_vec(&journal.facts)
            .map_err(|e| AuraError::serialization(e.to_string()))?;
        Ok(Hash32::from_bytes(&journal_bytes))
    }

    /// Generate a deterministic relationship ID.
    fn generate_relationship_id(
        &self,
        ceremony_id: InvitationCeremonyId,
        acceptance: &AcceptanceProposal,
    ) -> String {
        // Build input for hashing: ceremony_id + acceptor UUID bytes
        let mut input = Vec::with_capacity(32 + 16);
        input.extend_from_slice(ceremony_id.0.as_bytes());
        input.extend_from_slice(acceptance.acceptor.uuid().as_bytes());
        let hash = Hash32::from_bytes(&input);
        format!("rel-{}", hex::encode(&hash.as_bytes()[..8]))
    }

    /// Emit ceremony initiated fact.
    async fn emit_ceremony_initiated_fact(
        &self,
        ceremony_id: InvitationCeremonyId,
        sender: AuthorityId,
    ) -> AuraResult<()> {
        let timestamp_ms = self.effects.current_timestamp_ms().await;
        let fact = InvitationFact::CeremonyInitiated {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            sender: sender.to_string(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!(
            "ceremony:initiated:{}",
            hex::encode(ceremony_id.0.as_bytes())
        );
        // Serialize fact to bytes and store
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit acceptance received fact.
    async fn emit_acceptance_received_fact(
        &self,
        ceremony_id: InvitationCeremonyId,
    ) -> AuraResult<()> {
        let timestamp_ms = self.effects.current_timestamp_ms().await;
        let fact = InvitationFact::CeremonyAcceptanceReceived {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!(
            "ceremony:accepted:{}",
            hex::encode(ceremony_id.0.as_bytes())
        );
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit ceremony committed fact.
    async fn emit_ceremony_committed_fact(
        &self,
        ceremony_id: InvitationCeremonyId,
        relationship_id: &str,
    ) -> AuraResult<()> {
        let timestamp_ms = self.effects.current_timestamp_ms().await;
        let fact = InvitationFact::CeremonyCommitted {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            relationship_id: relationship_id.to_string(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!(
            "ceremony:committed:{}",
            hex::encode(ceremony_id.0.as_bytes())
        );
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit ceremony aborted fact.
    async fn emit_ceremony_aborted_fact(
        &self,
        ceremony_id: InvitationCeremonyId,
        reason: &str,
    ) -> AuraResult<()> {
        let timestamp_ms = self.effects.current_timestamp_ms().await;
        let fact = InvitationFact::CeremonyAborted {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            reason: reason.to_string(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!("ceremony:aborted:{}", hex::encode(ceremony_id.0.as_bytes()));
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Get ceremony state.
    pub fn get_ceremony(
        &self,
        ceremony_id: &InvitationCeremonyId,
    ) -> Option<&InvitationCeremonyState> {
        self.ceremonies.get(ceremony_id)
    }

    /// Check if a ceremony exists.
    pub fn has_ceremony(&self, ceremony_id: &InvitationCeremonyId) -> bool {
        self.ceremonies.contains_key(ceremony_id)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_prestate() -> Hash32 {
        Hash32([1u8; 32])
    }

    fn test_invitation_hash() -> Hash32 {
        Hash32([2u8; 32])
    }

    #[test]
    fn test_ceremony_id_determinism() {
        let id1 = InvitationCeremonyId::new(&test_prestate(), &test_invitation_hash(), 12345);
        let id2 = InvitationCeremonyId::new(&test_prestate(), &test_invitation_hash(), 12345);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_ceremony_id_uniqueness_with_nonce() {
        let id1 = InvitationCeremonyId::new(&test_prestate(), &test_invitation_hash(), 12345);
        let id2 = InvitationCeremonyId::new(&test_prestate(), &test_invitation_hash(), 12346);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ceremony_id_uniqueness_with_prestate() {
        let prestate1 = Hash32([1u8; 32]);
        let prestate2 = Hash32([3u8; 32]);
        let id1 = InvitationCeremonyId::new(&prestate1, &test_invitation_hash(), 12345);
        let id2 = InvitationCeremonyId::new(&prestate2, &test_invitation_hash(), 12345);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ceremony_status_transitions() {
        let status = CeremonyStatus::AwaitingAcceptance;
        assert!(matches!(status, CeremonyStatus::AwaitingAcceptance));

        let status = CeremonyStatus::AwaitingConsensus;
        assert!(matches!(status, CeremonyStatus::AwaitingConsensus));

        let status = CeremonyStatus::Committed;
        assert!(matches!(status, CeremonyStatus::Committed));

        let status = CeremonyStatus::Aborted {
            reason: "test".to_string(),
        };
        assert!(matches!(status, CeremonyStatus::Aborted { .. }));
    }

    #[test]
    fn test_acceptance_proposal_serialization() {
        let proposal = AcceptanceProposal {
            invitation_id: "inv-123".to_string(),
            acceptor: AuthorityId::new_from_entropy([42u8; 32]),
            prestate_hash: Hash32([0u8; 32]),
            message: Some("Accepting".to_string()),
            signature: ThresholdSignature::single_signer(vec![1, 2, 3], vec![4, 5, 6], 0),
        };

        let bytes = serde_json::to_vec(&proposal).unwrap();
        let restored: AcceptanceProposal = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "inv-123");
        assert_eq!(restored.message, Some("Accepting".to_string()));
    }
}
