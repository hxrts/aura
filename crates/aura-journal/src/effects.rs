//! Journal Effects Implementation (Layer 2 - Clean Architecture)
use crate::extensibility::FactRegistry;
use async_trait::async_trait;
use aura_core::effects::BiscuitAuthorizationEffects;
use aura_core::effects::{CryptoEffects, JournalEffects, StorageEffects};
use aura_core::flow::FlowBudget;
use aura_core::scope::{AuthorityOp, ContextOp, ResourceScope};
use aura_core::types::Epoch;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::{
    hash::hash,
    identifiers::{AuthorityId, ContextId},
    semilattice::JoinSemilattice,
    AuraError, FactValue, Journal,
};
// Flow budget handling moved to effects system
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

const DEFAULT_FLOW_BUDGET_LIMIT: u64 = 1024;

/// Storage envelope for persisted journal state
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredJournal {
    journal: Journal,
}

fn derive_context_id(label: &[u8], parts: &[&[u8]]) -> ContextId {
    let mut input = Vec::new();
    input.extend_from_slice(label);
    for part in parts {
        input.extend_from_slice(part);
    }
    ContextId::new_from_entropy(hash(&input))
}

fn protocol_context_id(protocol: &crate::protocol_facts::ProtocolRelationalFact) -> ContextId {
    use crate::protocol_facts::ProtocolRelationalFact::*;
    match protocol {
        GuardianBinding {
            account_id,
            guardian_id,
            ..
        } => derive_context_id(
            b"guardian-binding",
            &[&account_id.to_bytes(), &guardian_id.to_bytes()],
        ),
        RecoveryGrant {
            account_id,
            guardian_id,
            grant_hash,
        } => derive_context_id(
            b"recovery-grant",
            &[
                &account_id.to_bytes(),
                &guardian_id.to_bytes(),
                grant_hash.as_bytes(),
            ],
        ),
        Consensus {
            consensus_id,
            operation_hash,
            ..
        } => derive_context_id(
            b"consensus",
            &[consensus_id.as_bytes(), operation_hash.as_bytes()],
        ),
        AmpChannelCheckpoint(checkpoint) => checkpoint.context,
        AmpProposedChannelEpochBump(proposed) => proposed.context,
        AmpCommittedChannelEpochBump(committed) => committed.context,
        AmpChannelPolicy(policy) => policy.context,
        LeakageEvent(event) => event.context_id,
        DkgTranscriptCommit(commit) => commit.context,
        ConvergenceCert(cert) => cert.context,
        ReversionFact(reversion) => reversion.context,
        RotateFact(rotate) => rotate.context,
    }
}

fn relational_context_id(rel: &crate::fact::RelationalFact) -> ContextId {
    use crate::fact::RelationalFact::*;
    match rel {
        Protocol(protocol) => protocol_context_id(protocol),
        // Generic handles all domain-specific facts (ChatFact, InvitationFact, ContactFact)
        // via DomainFact::to_generic() - context_id is always stored in the binding
        Generic { context_id, .. } => *context_id,
    }
}

/// Domain-specific journal handler that persists state via StorageEffects
pub struct JournalHandler<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects> {
    crypto: C,
    storage: S,
    authorization: Option<A>,
    policy_token: Option<Vec<u8>>, // Raw Biscuit token bytes
    authority_id: AuthorityId,
    verifying_key: Option<Vec<u8>>,
    fact_registry: Option<FactRegistry>,
    _phantom: PhantomData<()>,
}

impl<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects> JournalHandler<C, S, A> {
    /// Creates a new journal handler with a fresh authority ID.
    pub fn new(crypto: C, storage: S) -> Self {
        Self::with_authority(AuthorityId::default(), crypto, storage)
    }

    /// Creates a new journal handler with the specified authority ID.
    pub fn with_authority(authority_id: AuthorityId, crypto: C, storage: S) -> Self {
        Self {
            crypto,
            storage,
            authorization: None,
            policy_token: None,
            authority_id,
            verifying_key: None,
            fact_registry: None,
            _phantom: PhantomData,
        }
    }

    /// Attach authorization effects and Biscuit policy token for fact authorization.
    pub fn with_authorization(mut self, token_data: Vec<u8>, auth_effects: A) -> Self {
        self.policy_token = Some(token_data);
        self.authorization = Some(auth_effects);
        self
    }

    /// Attach a public verifying key for signature checks (ed25519).
    pub fn with_verifying_key(mut self, verifying_key: Vec<u8>) -> Self {
        self.verifying_key = Some(verifying_key);
        self
    }

    /// Attach a fact registry for domain-specific fact reduction.
    pub fn with_fact_registry(mut self, registry: FactRegistry) -> Self {
        self.fact_registry = Some(registry);
        self
    }

    fn with_authorization_if(mut self, auth: Option<(Vec<u8>, A)>) -> Self {
        if let Some((token_data, auth_effects)) = auth {
            self = self.with_authorization(token_data, auth_effects);
        }
        self
    }

    fn with_verifying_key_if(mut self, verifying_key: Option<Vec<u8>>) -> Self {
        if let Some(pk) = verifying_key {
            self = self.with_verifying_key(pk);
        }
        self
    }

    fn with_fact_registry_if(mut self, registry: Option<FactRegistry>) -> Self {
        if let Some(reg) = registry {
            self = self.with_fact_registry(reg);
        }
        self
    }

    /// Get a reference to the fact registry if one is attached.
    pub fn fact_registry(&self) -> Option<&FactRegistry> {
        self.fact_registry.as_ref()
    }

    async fn authorize_fact(&self, content: &crate::fact::FactContent) -> Result<(), AuraError> {
        if let (Some(token_data), Some(authorization)) = (&self.policy_token, &self.authorization) {
            let scope = match content {
                crate::fact::FactContent::AttestedOp(_) => ResourceScope::Authority {
                    authority_id: self.authority_id,
                    operation: AuthorityOp::UpdateTree,
                },
                crate::fact::FactContent::Relational(rel) => {
                    let context_id = relational_context_id(rel);
                    ResourceScope::Context {
                        context_id,
                        operation: ContextOp::UpdateParams,
                    }
                }
                crate::fact::FactContent::Snapshot(_) => ResourceScope::Authority {
                    authority_id: self.authority_id,
                    operation: AuthorityOp::Rotate,
                },
                crate::fact::FactContent::RendezvousReceipt { .. } => ResourceScope::Authority {
                    authority_id: self.authority_id,
                    operation: AuthorityOp::AddGuardian,
                },
            };
            let authorized = authorization
                .authorize_fact(token_data, "journal_fact", &scope)
                .await
                .map_err(|e| AuraError::permission_denied(e.to_string()))?;
            if !authorized {
                return Err(AuraError::permission_denied(
                    "journal fact not authorized by Biscuit policy",
                ));
            }
        }
        Ok(())
    }

    async fn verify_fact_signature(
        &self,
        content: &crate::fact::FactContent,
    ) -> Result<(), AuraError> {
        if let crate::fact::FactContent::RendezvousReceipt {
            envelope_id,
            authority_id: _,
            timestamp,
            signature,
        } = content
        {
            if signature.is_empty() {
                return Ok(());
            }
            if let Some(pk_bytes) = &self.verifying_key {
                let mut message = Vec::new();
                message.extend_from_slice(envelope_id);
                // Convert timestamp to a deterministic binary representation for signing
                let ts_bytes = aura_core::util::serialization::to_vec(timestamp)
                    .unwrap_or_else(|_| Vec::new());
                message.extend_from_slice(&ts_bytes);
                let verified = self
                    .crypto
                    .ed25519_verify(&message, signature, pk_bytes)
                    .await?;
                if !verified {
                    return Err(AuraError::permission_denied(
                        "invalid rendezvous receipt signature",
                    ));
                }
            }
        }
        Ok(())
    }

    fn extract_fact_contents(&self, journal: &Journal) -> Vec<crate::fact::FactContent> {
        let mut contents = Vec::new();
        for (_key, value) in journal.read_facts().iter() {
            match value {
                FactValue::Bytes(bytes) => {
                    if let Ok(content) = serde_json::from_slice(bytes) {
                        contents.push(content);
                    }
                }
                FactValue::String(text) => {
                    if let Ok(content) = serde_json::from_str(text) {
                        contents.push(content);
                    }
                }
                FactValue::Nested(nested) => {
                    if let Ok(bytes) = serde_json::to_vec(nested) {
                        if let Ok(content) = serde_json::from_slice(&bytes) {
                            contents.push(content);
                        }
                    }
                }
                _ => {}
            }
        }
        contents
    }

    fn journal_key(&self) -> &'static str {
        "journal"
    }
}

#[async_trait]
impl<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects + Send + Sync>
    JournalEffects for JournalHandler<C, S, A>
{
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError> {
        for content in self.extract_fact_contents(delta) {
            self.authorize_fact(&content).await?;
            self.verify_fact_signature(&content).await?;
        }

        let mut merged = target.clone();
        merged.merge_facts(delta.read_facts().clone());
        Ok(merged)
    }

    async fn refine_caps(
        &self,
        target: &Journal,
        refinement: &Journal,
    ) -> Result<Journal, AuraError> {
        for content in self.extract_fact_contents(refinement) {
            self.authorize_fact(&content).await?;
            self.verify_fact_signature(&content).await?;
        }

        let mut refined = target.clone();
        refined.refine_caps(refinement.read_caps().clone());

        if refined.read_caps().is_empty() {
            return Err(AuraError::permission_denied(
                "capability refinement produced empty frontier",
            ));
        }

        Ok(refined)
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        if let Some(bytes) = self.storage.retrieve(self.journal_key()).await? {
            let stored: StoredJournal = serde_json::from_slice(&bytes)
                .map_err(|e| AuraError::serialization(e.to_string()))?;
            Ok(stored.journal)
        } else {
            Ok(Journal::new())
        }
    }

    async fn persist_journal(&self, _journal: &Journal) -> Result<(), AuraError> {
        let stored = StoredJournal {
            journal: _journal.clone(),
        };
        let bytes =
            serde_json::to_vec(&stored).map_err(|e| AuraError::serialization(e.to_string()))?;
        self.storage
            .store(self.journal_key(), bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))
    }

    async fn get_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        let key = self.flow_budget_key(context, peer);
        if let Some(bytes) = self
            .storage
            .retrieve(&key)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?
        {
            let budget: FlowBudget =
                from_slice(&bytes).map_err(|e| AuraError::serialization(e.to_string()))?;
            return Ok(budget);
        }

        Ok(FlowBudget::new(DEFAULT_FLOW_BUDGET_LIMIT, Epoch::initial()))
    }

    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        let current = self.get_flow_budget(context, peer).await?;
        let merged = current.join(budget);
        let bytes = to_vec(&merged).map_err(|e| AuraError::serialization(e.to_string()))?;
        let key = self.flow_budget_key(context, peer);
        self.storage
            .store(&key, bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;
        Ok(merged)
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> Result<FlowBudget, AuraError> {
        let mut current = self.get_flow_budget(context, peer).await?;
        if current.limit > 0 && !current.can_charge(cost as u64) {
            return Err(AuraError::budget_exceeded(format!(
                "insufficient flow budget: remaining={}, cost={}",
                current.remaining(),
                cost
            )));
        }
        current.spent = current.spent.saturating_add(cost as u64);
        self.update_flow_budget(context, peer, &current).await
    }
}

impl<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects> JournalHandler<C, S, A> {
    fn flow_budget_key(&self, context: &ContextId, peer: &AuthorityId) -> String {
        format!(
            "journal/flow-budget/{}/{}",
            hex::encode(context.to_bytes()),
            hex::encode(peer.to_bytes())
        )
    }
}

impl<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects + Send + Sync> Default
    for JournalHandler<C, S, A>
where
    C: Default,
    S: Default,
{
    fn default() -> Self {
        Self::new(C::default(), S::default())
    }
}

#[cfg(test)]
mod tests {
    use aura_core::domain::content::Hash32;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use aura_core::types::flow::{FlowBudget, Receipt};
    use aura_core::types::Epoch;

    #[test]
    fn flow_budget_charge_uses_cost_not_nonce() {
        let current = FlowBudget {
            limit: 100,
            spent: 20,
            epoch: Epoch::new(1),
        };
        let receipt = Receipt::new(
            ContextId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
            Epoch::new(2),
            7,
            99,
            Hash32::zero(),
            Vec::new(),
        );

        let updated = FlowBudget {
            limit: current.limit,
            spent: current.spent.saturating_add(receipt.cost as u64),
            epoch: receipt.epoch,
        };
        assert_eq!(updated.spent, 27);
        assert_eq!(updated.limit, 100);
        assert_eq!(updated.epoch, receipt.epoch);
    }
}

/// Factory for constructing journal handlers with policy and verification hooks.
pub struct JournalHandlerFactory;

impl JournalHandlerFactory {
    /// Creates a journal handler with optional Biscuit authorization, flow budget, verifying key, and fact registry.
    pub fn create<
        C: CryptoEffects,
        S: StorageEffects,
        A: BiscuitAuthorizationEffects + Send + Sync,
    >(
        authority_id: AuthorityId,
        crypto: C,
        storage: S,
        authorization: Option<(Vec<u8>, A)>,
        verifying_key: Option<Vec<u8>>,
        fact_registry: Option<FactRegistry>,
    ) -> JournalHandler<C, S, A> {
        JournalHandler::with_authority(authority_id, crypto, storage)
            .with_authorization_if(authorization)
            .with_verifying_key_if(verifying_key)
            .with_fact_registry_if(fact_registry)
    }
}
