//! Journal Effects Implementation (minimal stub for simulator wiring)
use async_trait::async_trait;
use aura_core::effects::{CryptoEffects, JournalEffects, StorageEffects};
use aura_core::FlowBudgetEffects;
use aura_core::{
    identifiers::{AuthorityId, ContextId},
    semilattice::JoinSemilattice,
    AuraError, FactValue, FlowBudget, Journal,
};
use aura_wot::BiscuitAuthorizationBridge;
use aura_wot::{AuthorityOp, ContextOp, FlowBudgetHandler, ResourceScope};
use bincode;
use biscuit_auth::Biscuit;
use futures::executor;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Storage envelope for persisted journal state
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredJournal {
    journal: Journal,
}

/// Domain-specific journal handler that persists state via StorageEffects
pub struct JournalHandler<C: CryptoEffects, S: StorageEffects> {
    crypto: C,
    storage: S,
    flow_handler: FlowBudgetHandler,
    biscuit_bridge: Option<BiscuitAuthorizationBridge>,
    policy_token: Option<Biscuit>,
    authority_id: AuthorityId,
    verifying_key: Option<Vec<u8>>,
    _phantom: PhantomData<()>,
}

impl<C: CryptoEffects, S: StorageEffects> JournalHandler<C, S> {
    /// Creates a new journal handler with a fresh authority ID.
    pub fn new(crypto: C, storage: S) -> Self {
        Self::with_authority(AuthorityId::new(), crypto, storage)
    }

    /// Creates a new journal handler with the specified authority ID.
    pub fn with_authority(authority_id: AuthorityId, crypto: C, storage: S) -> Self {
        Self {
            crypto,
            storage,
            flow_handler: FlowBudgetHandler::new(authority_id),
            biscuit_bridge: None,
            policy_token: None,
            authority_id,
            verifying_key: None,
            _phantom: PhantomData,
        }
    }

    /// Attach a Biscuit policy token and bridge for fact authorization.
    pub fn with_policy(mut self, token: Biscuit, bridge: BiscuitAuthorizationBridge) -> Self {
        self.policy_token = Some(token);
        self.biscuit_bridge = Some(bridge);
        self
    }

    /// Attach a public verifying key for signature checks (ed25519).
    pub fn with_verifying_key(mut self, verifying_key: Vec<u8>) -> Self {
        self.verifying_key = Some(verifying_key);
        self
    }

    fn with_policy_if(mut self, policy: Option<(Biscuit, BiscuitAuthorizationBridge)>) -> Self {
        if let Some((token, bridge)) = policy {
            self = self.with_policy(token, bridge);
        }
        self
    }

    fn with_verifying_key_if(mut self, verifying_key: Option<Vec<u8>>) -> Self {
        if let Some(pk) = verifying_key {
            self = self.with_verifying_key(pk);
        }
        self
    }

    fn authorize_fact(&self, content: &crate::fact::FactContent) -> Result<(), AuraError> {
        if let (Some(token), Some(bridge)) = (&self.policy_token, &self.biscuit_bridge) {
            let scope = match content {
                crate::fact::FactContent::AttestedOp(_) => ResourceScope::Authority {
                    authority_id: self.authority_id,
                    operation: AuthorityOp::UpdateTree,
                },
                crate::fact::FactContent::Relational(rel) => {
                    let context_id = match rel {
                        crate::fact::RelationalFact::GuardianBinding { .. }
                        | crate::fact::RelationalFact::RecoveryGrant { .. } => ContextId::new(),
                        crate::fact::RelationalFact::Consensus { .. } => ContextId::new(),
                        crate::fact::RelationalFact::AmpChannelCheckpoint(checkpoint) => {
                            checkpoint.context
                        }
                        crate::fact::RelationalFact::AmpProposedChannelEpochBump(proposed) => {
                            proposed.context
                        }
                        crate::fact::RelationalFact::AmpCommittedChannelEpochBump(committed) => {
                            committed.context
                        }
                        crate::fact::RelationalFact::AmpChannelPolicy(policy) => policy.context,
                        crate::fact::RelationalFact::Generic { context_id, .. } => *context_id,
                    };
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
            let allowed = bridge
                .authorize(token, "journal_fact", &scope)
                .map_err(|e| AuraError::permission_denied(e.to_string()))?;
            if !allowed.authorized {
                return Err(AuraError::permission_denied(
                    "journal fact not authorized by Biscuit policy",
                ));
            }
        }
        Ok(())
    }

    fn verify_fact_signature(&self, content: &crate::fact::FactContent) -> Result<(), AuraError> {
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
                let ts_bytes = bincode::serialize(timestamp).unwrap_or_else(|_| Vec::new());
                message.extend_from_slice(&ts_bytes);
                let verified = executor::block_on(async {
                    self.crypto
                        .ed25519_verify(&message, signature, pk_bytes)
                        .await
                })?;
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
impl<C: CryptoEffects, S: StorageEffects> JournalEffects for JournalHandler<C, S> {
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError> {
        for content in self.extract_fact_contents(delta) {
            self.authorize_fact(&content)?;
            self.verify_fact_signature(&content)?;
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
            self.authorize_fact(&content)?;
            self.verify_fact_signature(&content)?;
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
        _context: &ContextId,
        _peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        // Delegate to aura-wot flow handler to avoid duplicate budget logic
        self.flow_handler
            .charge_flow(_context, _peer, 0) // query as no-op charge
            .await
            .map(|receipt| FlowBudget {
                limit: 0,
                spent: receipt.nonce,
                epoch: receipt.epoch,
            })
            .or_else(|_| Ok(FlowBudget::default()))
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        // Mirror storage for compatibility but keep aura-wot as source of truth
        let merged = self.get_flow_budget(_context, _peer).await?.join(budget);
        let _ = self.flow_handler.charge_flow(_context, _peer, 0).await.ok();
        Ok(merged)
    }

    async fn charge_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        _cost: u32,
    ) -> Result<FlowBudget, AuraError> {
        let receipt = self
            .flow_handler
            .charge_flow(_context, _peer, _cost)
            .await?;
        Ok(FlowBudget {
            limit: 0,
            spent: receipt.nonce,
            epoch: receipt.epoch,
        })
    }
}

impl<C: CryptoEffects, S: StorageEffects> Default for JournalHandler<C, S>
where
    C: Default,
    S: Default,
{
    fn default() -> Self {
        Self::new(C::default(), S::default())
    }
}

/// Factory for constructing journal handlers with policy and verification hooks.
pub struct JournalHandlerFactory;

impl JournalHandlerFactory {
    /// Creates a journal handler with optional Biscuit policy and verifying key.
    pub fn create<C: CryptoEffects, S: StorageEffects>(
        authority_id: AuthorityId,
        crypto: C,
        storage: S,
        policy: Option<(Biscuit, BiscuitAuthorizationBridge)>,
        verifying_key: Option<Vec<u8>>,
    ) -> JournalHandler<C, S> {
        JournalHandler::with_authority(authority_id, crypto, storage)
            .with_policy_if(policy)
            .with_verifying_key_if(verifying_key)
    }
}
