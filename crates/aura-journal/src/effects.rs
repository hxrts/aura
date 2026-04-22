//! Journal Effects Implementation (Layer 2 - Clean Architecture)
use crate::extensibility::FactRegistry;
use async_trait::async_trait;
use aura_core::effects::BiscuitAuthorizationEffects;
use aura_core::effects::{CryptoEffects, JournalEffects, StorageEffects};
use aura_core::types::flow::{FlowBudget, FlowCost};
use aura_core::types::scope::{AuthorityOp, ContextOp, ResourceScope};
use aura_core::types::Epoch;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::{
    semilattice::JoinSemilattice,
    types::identifiers::{AuthorityId, ContextId},
    AuraError, FactValue, Journal,
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

const DEFAULT_FLOW_BUDGET_LIMIT: u64 = 1024;

/// Storage envelope for persisted journal state
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredJournal {
    journal: Journal,
}

/// Domain-specific journal handler that persists state via StorageEffects
pub struct JournalHandler<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects> {
    crypto: C,
    storage: S,
    authorization: JournalAuthorizationMode<A>,
    authority_id: AuthorityId,
    verifying_key: Option<Vec<u8>>,
    fact_registry: Option<FactRegistry>,
    _phantom: PhantomData<()>,
}

enum JournalAuthorizationMode<A: BiscuitAuthorizationEffects> {
    Production {
        token_data: Vec<u8>,
        effects: A,
    },
    #[cfg(test)]
    TestSimulationBypass {
        reason: &'static str,
    },
}

impl<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects> JournalHandler<C, S, A> {
    fn with_authorization_mode(
        authority_id: AuthorityId,
        crypto: C,
        storage: S,
        authorization: JournalAuthorizationMode<A>,
    ) -> Self {
        Self {
            crypto,
            storage,
            authorization,
            authority_id,
            verifying_key: None,
            fact_registry: None,
            _phantom: PhantomData,
        }
    }

    /// Creates a journal handler with an explicit test/simulation authorization bypass.
    #[cfg(test)]
    pub fn new_for_test_with_authorization_bypass(
        authority_id: AuthorityId,
        crypto: C,
        storage: S,
        reason: &'static str,
    ) -> Self {
        Self::with_authorization_mode(
            authority_id,
            crypto,
            storage,
            JournalAuthorizationMode::TestSimulationBypass { reason },
        )
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

    /// Get a reference to the fact registry if one is attached.
    pub fn fact_registry(&self) -> Option<&FactRegistry> {
        self.fact_registry.as_ref()
    }

    async fn authorize_fact(&self, content: &crate::fact::FactContent) -> Result<(), AuraError> {
        match &self.authorization {
            JournalAuthorizationMode::Production {
                token_data,
                effects,
            } => {
                let scope = match content {
                    crate::fact::FactContent::AttestedOp(_) => ResourceScope::Authority {
                        authority_id: self.authority_id,
                        operation: AuthorityOp::UpdateTree,
                    },
                    crate::fact::FactContent::Relational(rel) => ResourceScope::Context {
                        context_id: rel.context_id(),
                        operation: ContextOp::UpdateParams,
                    },
                    crate::fact::FactContent::Snapshot(_) => ResourceScope::Authority {
                        authority_id: self.authority_id,
                        operation: AuthorityOp::Rotate,
                    },
                    crate::fact::FactContent::RendezvousReceipt { .. } => {
                        ResourceScope::Authority {
                            authority_id: self.authority_id,
                            operation: AuthorityOp::AddGuardian,
                        }
                    }
                };
                let authorized = effects
                    .authorize_fact(token_data, "journal_fact", &scope)
                    .await
                    .map_err(|e| AuraError::permission_denied(e.to_string()))?;
                if !authorized {
                    return Err(AuraError::permission_denied(
                        "journal fact not authorized by Biscuit policy",
                    ));
                }
            }
            #[cfg(test)]
            JournalAuthorizationMode::TestSimulationBypass { reason } => {
                tracing::debug!(reason, "journal authorization bypassed for test/simulation")
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

    fn decode_fact_content(value: &FactValue) -> Option<crate::fact::FactContent> {
        match value {
            FactValue::Bytes(bytes) => serde_json::from_slice(bytes).ok(),
            FactValue::String(text) => serde_json::from_str(text).ok(),
            FactValue::Nested(nested) => serde_json::to_vec(nested)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok()),
            _ => None,
        }
    }

    fn extract_fact_contents(&self, journal: &Journal) -> Vec<crate::fact::FactContent> {
        journal
            .read_facts()
            .iter()
            .map(|(_key, value)| value)
            .filter_map(Self::decode_fact_content)
            .collect()
    }

    fn journal_key(&self) -> &'static str {
        "journal"
    }
}

#[async_trait]
impl<C: CryptoEffects, S: StorageEffects, A: BiscuitAuthorizationEffects + Send + Sync>
    JournalEffects for JournalHandler<C, S, A>
{
    async fn merge_facts(&self, mut target: Journal, delta: Journal) -> Result<Journal, AuraError> {
        for content in self.extract_fact_contents(&delta) {
            self.authorize_fact(&content).await?;
            self.verify_fact_signature(&content).await?;
        }

        target.merge_facts(delta.facts);
        Ok(target)
    }

    async fn refine_caps(
        &self,
        mut target: Journal,
        refinement: Journal,
    ) -> Result<Journal, AuraError> {
        for content in self.extract_fact_contents(&refinement) {
            self.authorize_fact(&content).await?;
            self.verify_fact_signature(&content).await?;
        }

        target.refine_caps(refinement.caps);

        if target.read_caps().is_empty() {
            return Err(AuraError::permission_denied(
                "capability refinement produced empty frontier",
            ));
        }

        Ok(target)
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
        cost: FlowCost,
    ) -> Result<FlowBudget, AuraError> {
        let mut current = self.get_flow_budget(context, peer).await?;
        if current.limit > 0 {
            current
                .record_charge(cost)
                .map_err(|e| AuraError::budget_exceeded(e.to_string()))?;
        } else {
            let cost_value = u64::from(cost);
            current.spent = current.spent.checked_add(cost_value).ok_or_else(|| {
                AuraError::invalid("flow budget overflow while recording unbounded spend")
            })?;
        }
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

/// Factory for constructing journal handlers with policy and verification hooks.
pub struct JournalHandlerFactory;

impl JournalHandlerFactory {
    /// Creates a journal handler with required Biscuit authorization, optional verifying key, and fact registry.
    pub fn create<
        C: CryptoEffects,
        S: StorageEffects,
        A: BiscuitAuthorizationEffects + Send + Sync,
    >(
        authority_id: AuthorityId,
        crypto: C,
        storage: S,
        authorization: (Vec<u8>, A),
        verifying_key: Option<Vec<u8>>,
        fact_registry: Option<FactRegistry>,
    ) -> JournalHandler<C, S, A> {
        let (token_data, auth_effects) = authorization;
        let mut handler = JournalHandler::with_authorization_mode(
            authority_id,
            crypto,
            storage,
            JournalAuthorizationMode::Production {
                token_data,
                effects: auth_effects,
            },
        );
        if let Some(pk) = verifying_key {
            handler = handler.with_verifying_key(pk);
        }
        if let Some(registry) = fact_registry {
            handler = handler.with_fact_registry(registry);
        }
        handler
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::fact::{AttestedOp, FactContent, RelationalFact, SnapshotFact, TreeOpKind};
    use aura_core::effects::crypto::KeyDerivationContext;
    use aura_core::effects::{
        AuthorizationDecision, AuthorizationError, CryptoCoreEffects, CryptoError,
        CryptoExtendedEffects, RandomCoreEffects, StorageCoreEffects, StorageError,
        StorageExtendedEffects, StorageStats,
    };
    use aura_core::time::{PhysicalTime, TimeStamp};
    use aura_core::tree::LeafRole;
    use aura_core::types::facts::{FactEncoding, FactEnvelope, FactTypeId};
    use aura_core::{AuthorizationOp, Cap, Fact, FactValue, Hash32};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct TestCrypto;

    #[async_trait]
    impl RandomCoreEffects for TestCrypto {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![7; len]
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            [7; 32]
        }

        async fn random_u64(&self) -> u64 {
            7
        }
    }

    #[async_trait]
    impl CryptoCoreEffects for TestCrypto {
        async fn kdf_derive(
            &self,
            _ikm: &[u8],
            _salt: &[u8],
            _info: &[u8],
            output_len: u32,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![0; output_len as usize])
        }

        async fn derive_key(
            &self,
            _master_key: &[u8],
            _context: &KeyDerivationContext,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![0; 32])
        }

        async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
            Ok((vec![1; 32], vec![2; 32]))
        }

        async fn ed25519_sign(
            &self,
            _message: &[u8],
            _private_key: &[u8],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![3; 64])
        }

        async fn ed25519_verify(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _public_key: &[u8],
        ) -> Result<bool, CryptoError> {
            Ok(true)
        }

        fn is_simulated(&self) -> bool {
            true
        }

        fn crypto_capabilities(&self) -> Vec<String> {
            vec!["test".to_string()]
        }

        fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
            a == b
        }

        fn secure_zero(&self, data: &mut [u8]) {
            data.fill(0);
        }
    }

    #[async_trait]
    impl CryptoExtendedEffects for TestCrypto {}

    #[derive(Clone, Default)]
    struct TestStorage {
        data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    #[async_trait]
    impl StorageCoreEffects for TestStorage {
        async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
            self.data
                .lock()
                .expect("test storage lock")
                .insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(self
                .data
                .lock()
                .expect("test storage lock")
                .get(key)
                .cloned())
        }

        async fn remove(&self, key: &str) -> Result<bool, StorageError> {
            Ok(self
                .data
                .lock()
                .expect("test storage lock")
                .remove(key)
                .is_some())
        }

        async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            let data = self.data.lock().expect("test storage lock");
            Ok(data
                .keys()
                .filter(|key| prefix.is_none_or(|prefix| key.starts_with(prefix)))
                .cloned()
                .collect())
        }
    }

    #[async_trait]
    impl StorageExtendedEffects for TestStorage {
        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            Ok(self
                .data
                .lock()
                .expect("test storage lock")
                .contains_key(key))
        }

        async fn stats(&self) -> Result<StorageStats, StorageError> {
            let data = self.data.lock().expect("test storage lock");
            Ok(StorageStats {
                key_count: data.len() as u64,
                total_size: data.values().map(|value| value.len() as u64).sum(),
                available_space: None,
                backend_type: "test".to_string(),
            })
        }
    }

    #[derive(Clone)]
    struct TestAuthorization {
        allow: bool,
    }

    #[async_trait]
    impl BiscuitAuthorizationEffects for TestAuthorization {
        async fn authorize_biscuit(
            &self,
            _token_data: &[u8],
            _operation: AuthorizationOp,
            _scope: &ResourceScope,
        ) -> Result<AuthorizationDecision, AuthorizationError> {
            Ok(AuthorizationDecision {
                authorized: self.allow,
                reason: None,
            })
        }

        async fn authorize_fact(
            &self,
            _token_data: &[u8],
            _fact_type: &str,
            _scope: &ResourceScope,
        ) -> Result<bool, AuthorizationError> {
            Ok(self.allow)
        }
    }

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn test_handler(allow: bool) -> JournalHandler<TestCrypto, TestStorage, TestAuthorization> {
        JournalHandlerFactory::create(
            authority(1),
            TestCrypto,
            TestStorage::default(),
            (b"policy-token".to_vec(), TestAuthorization { allow }),
            None,
            None,
        )
    }

    fn fact_journal(content: FactContent) -> Journal {
        let value = FactValue::Bytes(serde_json::to_vec(&content).expect("fact content encodes"));
        Journal::with_facts(Fact::with_value("fact", value).expect("journal fact builds"))
    }

    fn non_empty_cap() -> Cap {
        serde_json::from_value(serde_json::json!({
            "token_bytes": [1, 2, 3],
            "root_key_bytes": []
        }))
        .expect("test cap decodes")
    }

    fn attested_content() -> FactContent {
        FactContent::AttestedOp(AttestedOp {
            tree_op: TreeOpKind::AddLeaf {
                public_key: vec![1; 32],
                role: LeafRole::Device,
            },
            parent_commitment: Hash32::zero(),
            new_commitment: Hash32::new([1; 32]),
            witness_threshold: 1,
            signature: vec![9; 64],
        })
    }

    fn relational_content() -> FactContent {
        FactContent::Relational(RelationalFact::Generic {
            context_id: context(2),
            envelope: FactEnvelope {
                type_id: FactTypeId::new("journal-test/v1"),
                schema_version: 1,
                encoding: FactEncoding::Json,
                payload: br#"{"ok":true}"#.to_vec(),
            },
        })
    }

    fn snapshot_content() -> FactContent {
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::new([2; 32]),
            superseded_facts: Vec::new(),
            sequence: 1,
        })
    }

    fn rendezvous_receipt_content() -> FactContent {
        FactContent::RendezvousReceipt {
            envelope_id: [3; 32],
            authority_id: authority(3),
            timestamp: TimeStamp::PhysicalClock(PhysicalTime::exact(1_000)),
            signature: Vec::new(),
        }
    }

    #[tokio::test]
    async fn production_authorization_denies_all_security_critical_fact_kinds() {
        let handler = test_handler(false);
        for content in [
            attested_content(),
            relational_content(),
            snapshot_content(),
            rendezvous_receipt_content(),
        ] {
            let result = handler
                .merge_facts(Journal::new(), fact_journal(content))
                .await;
            assert!(result.is_err(), "unauthorized fact should be denied");
        }
    }

    #[tokio::test]
    async fn production_authorization_allows_merge_and_refinement_with_valid_policy() {
        let handler = test_handler(true);
        let merged = handler
            .merge_facts(Journal::new(), fact_journal(attested_content()))
            .await
            .expect("authorized merge succeeds");
        assert!(!merged.read_facts().is_empty());

        let cap = non_empty_cap();
        let target = Journal::with_caps(cap.clone());
        let mut refinement = fact_journal(relational_content());
        refinement.caps = cap;

        let refined = handler
            .refine_caps(target, refinement)
            .await
            .expect("authorized refinement succeeds");
        assert!(!refined.read_caps().is_empty());
    }

    #[tokio::test]
    async fn explicit_test_simulation_bypass_allows_fact_merge() {
        let handler = JournalHandler::<TestCrypto, TestStorage, TestAuthorization>::new_for_test_with_authorization_bypass(
            authority(4),
            TestCrypto,
            TestStorage::default(),
            "unit test verifies bypass is explicit",
        );
        let merged = handler
            .merge_facts(Journal::new(), fact_journal(attested_content()))
            .await
            .expect("explicit test bypass permits merge");
        assert!(!merged.read_facts().is_empty());
    }
}
