//! Guardian Setup Choreography
//!
//! Initial establishment of guardian relationships for a threshold account.
//! Uses the authority model - guardians are identified by AuthorityId.

use crate::{
    coordinator::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator},
    effects::RecoveryEffects,
    facts::{RecoveryFact, RecoveryFactEmitter},
    types::{GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    utils::EvidenceBuilder,
    RecoveryResult,
};
use async_trait::async_trait;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects};
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_journal::DomainFact;
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Guardian setup invitation data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInvitation {
    /// Unique identifier for this setup ceremony
    pub setup_id: String,
    /// Account authority being set up
    pub account_id: AuthorityId,
    /// Target guardian authorities
    pub target_guardians: Vec<AuthorityId>,
    /// Required threshold
    pub threshold: usize,
    /// Timestamp of invitation
    pub timestamp: TimeStamp,
}

/// Guardian acceptance of setup invitation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAcceptance {
    /// Guardian's authority
    pub guardian_id: AuthorityId,
    /// Setup ID being accepted
    pub setup_id: String,
    /// Whether the guardian accepted
    pub accepted: bool,
    /// Guardian's public key for this relationship
    pub public_key: Vec<u8>,
    /// Timestamp of acceptance
    pub timestamp: TimeStamp,
}

/// Setup completion notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupCompletion {
    /// Setup ceremony ID
    pub setup_id: String,
    /// Whether setup succeeded
    pub success: bool,
    /// Final guardian set
    pub guardian_set: GuardianSet,
    /// Final threshold
    pub threshold: usize,
}

// Guardian Setup Choreography - 3 phase protocol
choreography! {
    #[namespace = "guardian_setup"]
    protocol GuardianSetup {
        roles: SetupInitiator, Guardian1, Guardian2, Guardian3;

        // Phase 1: Send invitations to all guardians
        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300,
                       journal_facts = "guardian_setup_initiated",
                       leakage_budget = [1, 0, 0]]
        -> Guardian1: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300]
        -> Guardian2: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300]
        -> Guardian3: SendInvitation(GuardianInvitation);

        // Phase 2: Guardians respond with acceptance
        Guardian1[guard_capability = "accept_guardian_invitation,verify_setup_invitation",
                  flow_cost = 200,
                  journal_facts = "guardian_setup_accepted",
                  leakage_budget = [0, 1, 0]]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian2[guard_capability = "accept_guardian_invitation,verify_setup_invitation",
                  flow_cost = 200,
                  journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian3[guard_capability = "accept_guardian_invitation,verify_setup_invitation",
                  flow_cost = 200,
                  journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        // Phase 3: Broadcast completion to all guardians
        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_facts = "guardian_setup_completed",
                       journal_merge = true]
        -> Guardian1: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_merge = true]
        -> Guardian2: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_merge = true]
        -> Guardian3: CompleteSetup(SetupCompletion);
    }
}

/// Guardian setup coordinator.
///
/// Stateless coordinator that derives state from facts.
pub struct GuardianSetupCoordinator<E: RecoveryEffects> {
    base: BaseCoordinator<E>,
}

impl<E: RecoveryEffects> BaseCoordinatorAccess<E> for GuardianSetupCoordinator<E> {
    fn base(&self) -> &BaseCoordinator<E> {
        &self.base
    }
}

#[async_trait]
impl<E: RecoveryEffects + 'static> RecoveryCoordinator<E> for GuardianSetupCoordinator<E> {
    type Request = RecoveryRequest;
    type Response = RecoveryResponse;

    fn effect_system(&self) -> &Arc<E> {
        self.base_effect_system()
    }

    fn operation_name(&self) -> &str {
        "guardian_setup"
    }

    async fn execute_recovery(&self, request: Self::Request) -> RecoveryResult<Self::Response> {
        self.execute_setup(request).await
    }
}

impl<E: RecoveryEffects + 'static> GuardianSetupCoordinator<E> {
    /// Create a new coordinator.
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            base: BaseCoordinator::new(effect_system),
        }
    }

    /// Emit a recovery fact to the journal.
    async fn emit_fact(&self, fact: RecoveryFact) -> RecoveryResult<()> {
        let timestamp = self
            .effect_system()
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        let mut journal = self.effect_system().get_journal().await?;
        journal.facts.insert_with_context(
            RecoveryFactEmitter::fact_key(&fact),
            aura_core::FactValue::Bytes(DomainFact::to_bytes(&fact)),
            fact.context_id().to_string(),
            timestamp,
            None,
        );
        self.effect_system().persist_journal(&journal).await?;
        Ok(())
    }

    /// Execute guardian setup ceremony.
    pub async fn execute_setup(
        &self,
        request: RecoveryRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        // Get current timestamp for unique ID generation
        let now_ms = self
            .effect_system()
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        // Create context ID for this setup ceremony using hash of account + timestamp
        let setup_id = format!("setup_{}_{}", request.account_id, now_ms);
        let context_id = ContextId::new_from_entropy(hash::hash(setup_id.as_bytes()));

        // Emit GuardianSetupInitiated fact
        let guardian_ids: Vec<AuthorityId> =
            request.guardians.iter().map(|g| g.authority_id).collect();

        let initiated_fact = RecoveryFact::GuardianSetupInitiated {
            context_id,
            initiator_id: request.initiator_id,
            guardian_ids: guardian_ids.clone(),
            threshold: request.threshold as u16,
            initiated_at: PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        };
        self.emit_fact(initiated_fact).await?;

        // Validate that we have guardians
        if request.guardians.is_empty() {
            let failed_fact = RecoveryFact::GuardianSetupFailed {
                context_id,
                reason: "No guardians specified".to_string(),
                failed_at: PhysicalTime {
                    ts_ms: now_ms,
                    uncertainty: None,
                },
            };
            let _ = self.emit_fact(failed_fact).await;
            return Ok(RecoveryResponse::error("No guardians specified"));
        }

        // Create invitation
        let invitation = GuardianInvitation {
            setup_id: setup_id.clone(),
            account_id: request.account_id,
            target_guardians: guardian_ids.clone(),
            threshold: request.threshold,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            }),
        };

        // Execute the choreographic protocol (simulated)
        let acceptances = self.execute_choreographic_setup(invitation).await?;

        // Check if we have enough acceptances
        if acceptances.len() < request.threshold {
            let failed_fact = RecoveryFact::GuardianSetupFailed {
                context_id,
                reason: format!(
                    "Insufficient guardian acceptances: got {}, need {}",
                    acceptances.len(),
                    request.threshold
                ),
                failed_at: self
                    .effect_system()
                    .physical_time()
                    .await
                    .unwrap_or(PhysicalTime {
                        ts_ms: 0,
                        uncertainty: None,
                    }),
            };
            let _ = self.emit_fact(failed_fact).await;

            return Ok(RecoveryResponse::error(format!(
                "Insufficient guardian acceptances: got {}, need {}",
                acceptances.len(),
                request.threshold
            )));
        }

        // Create shares from acceptances
        let shares: Vec<RecoveryShare> = acceptances
            .iter()
            .map(|a| RecoveryShare {
                guardian_id: a.guardian_id,
                guardian_label: None,
                share: a.public_key.clone(),
                partial_signature: hash::hash(&a.public_key).to_vec(),
                issued_at_ms: now_ms,
            })
            .collect();

        // Emit completion fact
        let completed_fact = RecoveryFact::GuardianSetupCompleted {
            context_id,
            guardian_ids: shares.iter().map(|s| s.guardian_id).collect(),
            threshold: request.threshold as u16,
            completed_at: self
                .effect_system()
                .physical_time()
                .await
                .unwrap_or(PhysicalTime {
                    ts_ms: 0,
                    uncertainty: None,
                }),
        };
        self.emit_fact(completed_fact).await?;

        // Create evidence
        let evidence = EvidenceBuilder::success(context_id, request.account_id, &shares, now_ms);

        Ok(BaseCoordinator::<E>::success_response(
            None, shares, evidence,
        ))
    }

    /// Execute as guardian (accept setup invitation).
    pub async fn accept_as_guardian(
        &self,
        invitation: GuardianInvitation,
        guardian_id: AuthorityId,
    ) -> RecoveryResult<GuardianAcceptance> {
        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

        // Generate public key for this relationship
        let public_key =
            hash::hash(format!("{}_{}", invitation.setup_id, guardian_id).as_bytes()).to_vec();

        // Emit GuardianAccepted fact
        let context_id = ContextId::new_from_entropy(hash::hash(invitation.setup_id.as_bytes()));
        let accepted_fact = RecoveryFact::GuardianAccepted {
            context_id,
            guardian_id,
            accepted_at: physical_time.clone(),
        };
        self.emit_fact(accepted_fact).await?;

        Ok(GuardianAcceptance {
            guardian_id,
            setup_id: invitation.setup_id,
            accepted: true,
            public_key,
            timestamp: TimeStamp::PhysicalClock(physical_time),
        })
    }

    /// Execute choreographic setup protocol (Phase 1-2).
    async fn execute_choreographic_setup(
        &self,
        invitation: GuardianInvitation,
    ) -> RecoveryResult<Vec<GuardianAcceptance>> {
        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Time error: {}", e)))?;

        // Simulate guardian acceptances
        let mut acceptances = Vec::new();
        for guardian_id in &invitation.target_guardians {
            let public_key =
                hash::hash(format!("{}_{}", invitation.setup_id, guardian_id).as_bytes()).to_vec();

            acceptances.push(GuardianAcceptance {
                guardian_id: *guardian_id,
                setup_id: invitation.setup_id.clone(),
                accepted: true,
                public_key,
                timestamp: TimeStamp::PhysicalClock(physical_time.clone()),
            });
        }

        Ok(acceptances)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuardianProfile;
    use aura_testkit::MockEffects;
    use std::sync::Arc;

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn create_test_request() -> crate::types::RecoveryRequest {
        let guardians = vec![
            GuardianProfile::with_label(test_authority_id(1), "Guardian 1".to_string()),
            GuardianProfile::with_label(test_authority_id(2), "Guardian 2".to_string()),
            GuardianProfile::with_label(test_authority_id(3), "Guardian 3".to_string()),
        ];

        crate::types::RecoveryRequest {
            initiator_id: test_authority_id(0),
            account_id: test_authority_id(10),
            context: aura_authenticate::RecoveryContext {
                operation_type: aura_authenticate::RecoveryOperationType::DeviceKeyRecovery,
                justification: "Test recovery".to_string(),
                is_emergency: false,
                timestamp: 0,
            },
            threshold: 2,
            guardians: crate::types::GuardianSet::new(guardians),
        }
    }

    #[tokio::test]
    async fn test_guardian_setup_coordinator_creation() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        assert_eq!(coordinator.operation_name(), "guardian_setup");
    }

    #[tokio::test]
    async fn test_guardian_setup_execute() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        let request = create_test_request();
        let response = coordinator.execute_setup(request).await;

        assert!(response.is_ok());
        let resp = response.unwrap();
        assert!(resp.success);
        assert!(!resp.guardian_shares.is_empty());
    }

    #[tokio::test]
    async fn test_guardian_setup_empty_guardians() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        let mut request = create_test_request();
        request.guardians = crate::types::GuardianSet::new(vec![]);

        let response = coordinator.execute_setup(request).await;

        assert!(response.is_ok());
        let resp = response.unwrap();
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_accept_as_guardian() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        let invitation = GuardianInvitation {
            setup_id: "test-setup-123".to_string(),
            account_id: test_authority_id(10),
            target_guardians: vec![test_authority_id(1), test_authority_id(2)],
            threshold: 2,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };

        let guardian_id = test_authority_id(1);
        let acceptance = coordinator
            .accept_as_guardian(invitation, guardian_id)
            .await;

        assert!(acceptance.is_ok());
        let acc = acceptance.unwrap();
        assert!(acc.accepted);
        assert_eq!(acc.guardian_id, guardian_id);
        assert!(!acc.public_key.is_empty());
    }
}
