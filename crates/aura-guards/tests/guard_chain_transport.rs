#![allow(missing_docs)]
use async_trait::async_trait;
use aura_core::effects::authorization::AuthorizationError;
use aura_core::effects::guard::{EffectCommand, EffectInterpreter, EffectResult};
use aura_core::effects::leakage::{LeakageBudget, LeakageEvent, ObserverClass};
use aura_core::effects::storage::{StorageError, StorageStats};
use aura_core::effects::time::TimeError;
use aura_core::effects::{
    AuthorizationEffects, FlowBudgetEffects, JournalEffects, LeakageEffects, PhysicalTimeEffects,
    RandomCoreEffects, StorageCoreEffects, StorageExtendedEffects,
};
use aura_core::time::PhysicalTime;
use aura_core::types::Epoch;
use aura_core::types::flow::Receipt;
use aura_core::{AuraError, AuraResult, Cap, FlowBudget, Journal};
use aura_core::{AuthorityId, ContextId};
use aura_guards::executor::{execute_guard_plan, GuardPlan};
use aura_guards::guards::pure::GuardRequest;
use aura_guards::guards::traits::GuardContextProvider;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

struct CountingInterpreter {
    send_count: Arc<AtomicUsize>,
}

#[async_trait]
impl EffectInterpreter for CountingInterpreter {
    async fn execute(&self, cmd: EffectCommand) -> AuraResult<EffectResult> {
        match cmd {
            EffectCommand::SendEnvelope { .. } => {
                self.send_count.fetch_add(1, Ordering::SeqCst);
                Ok(EffectResult::Success)
            }
            _ => Ok(EffectResult::Success),
        }
    }

    fn interpreter_type(&self) -> &'static str {
        "CountingInterpreter"
    }
}

#[derive(Default)]
struct TestEffects {
    authority_id: AuthorityId,
    storage: Mutex<HashMap<String, Vec<u8>>>,
    journal: Mutex<Journal>,
    flow_budget: Mutex<FlowBudget>,
    nonce: AtomicU64,
    time_ms: AtomicU64,
}

impl TestEffects {
    fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            storage: Mutex::new(HashMap::new()),
            journal: Mutex::new(Journal::new()),
            flow_budget: Mutex::new(FlowBudget::new(1_000, Epoch::from(1))),
            nonce: AtomicU64::new(0),
            time_ms: AtomicU64::new(1_700_000_000_000),
        }
    }
}

impl GuardContextProvider for TestEffects {
    fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    fn get_metadata(&self, _key: &str) -> Option<String> {
        None
    }
}

#[async_trait]
impl PhysicalTimeEffects for TestEffects {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        Ok(PhysicalTime {
            ts_ms: self.time_ms.load(Ordering::SeqCst),
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        self.time_ms.fetch_add(ms, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl RandomCoreEffects for TestEffects {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let base = self.nonce.fetch_add(1, Ordering::SeqCst);
        (0..len)
            .map(|i| (base as u8).wrapping_add(i as u8))
            .collect()
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self.random_bytes(32).await;
        bytes.try_into().unwrap_or([0u8; 32])
    }

    async fn random_u64(&self) -> u64 {
        self.nonce.fetch_add(1, Ordering::SeqCst)
    }
}

#[async_trait]
impl StorageCoreEffects for TestEffects {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.storage.lock().insert(key.to_string(), value);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.storage.lock().get(key).cloned())
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.storage.lock().remove(key).is_some())
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let store = self.storage.lock();
        let mut keys: Vec<String> = store.keys().cloned().collect();
        if let Some(prefix) = prefix {
            keys.retain(|k| k.starts_with(prefix));
        }
        Ok(keys)
    }
}

#[async_trait]
impl StorageExtendedEffects for TestEffects {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.storage.lock().contains_key(key))
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut store = self.storage.lock();
        for (k, v) in pairs {
            store.insert(k, v);
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let store = self.storage.lock();
        let mut out = HashMap::new();
        for key in keys {
            if let Some(val) = store.get(key) {
                out.insert(key.clone(), val.clone());
            }
        }
        Ok(out)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        self.storage.lock().clear();
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let store = self.storage.lock();
        let total_size: u64 = store.values().map(|v| v.len() as u64).sum();
        Ok(StorageStats {
            key_count: store.len() as u64,
            total_size,
            available_space: None,
            backend_type: "test".to_string(),
        })
    }
}

#[async_trait]
impl JournalEffects for TestEffects {
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError> {
        let mut merged = target.clone();
        merged.merge(delta);
        Ok(merged)
    }

    async fn refine_caps(
        &self,
        target: &Journal,
        refinement: &Journal,
    ) -> Result<Journal, AuraError> {
        let mut merged = target.clone();
        merged.merge(refinement);
        Ok(merged)
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        Ok(self.journal.lock().clone())
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
        *self.journal.lock() = journal.clone();
        Ok(())
    }

    async fn get_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        Ok(*self.flow_budget.lock())
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        *self.flow_budget.lock() = *budget;
        Ok(*budget)
    }

    async fn charge_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        cost: u32,
    ) -> Result<FlowBudget, AuraError> {
        let mut budget = self.flow_budget.lock();
        if budget.can_charge(cost as u64) {
            budget.record_charge(cost as u64);
            Ok(*budget)
        } else {
            Err(AuraError::permission_denied("insufficient budget"))
        }
    }
}

#[async_trait]
impl FlowBudgetEffects for TestEffects {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> AuraResult<Receipt> {
        let _ = self.charge_flow_budget(context, peer, cost).await?;
        let nonce = self.nonce.fetch_add(1, Ordering::SeqCst);
        Ok(Receipt::new(
            *context,
            self.authority_id,
            *peer,
            Epoch::from(1),
            cost,
            nonce,
            aura_core::Hash32::default(),
            Vec::new(),
        ))
    }
}

#[async_trait]
impl AuthorizationEffects for TestEffects {
    async fn verify_capability(
        &self,
        _capabilities: &Cap,
        _operation: aura_core::AuthorizationOp,
        _scope: &aura_core::ResourceScope,
    ) -> Result<bool, AuthorizationError> {
        Ok(true)
    }

    async fn delegate_capabilities(
        &self,
        _source_capabilities: &Cap,
        requested_capabilities: &Cap,
        _target_authority: &AuthorityId,
    ) -> Result<Cap, AuthorizationError> {
        Ok(requested_capabilities.clone())
    }
}

#[async_trait]
impl LeakageEffects for TestEffects {
    async fn record_leakage(&self, _event: LeakageEvent) -> AuraResult<()> {
        Ok(())
    }

    async fn get_leakage_budget(&self, _context_id: ContextId) -> AuraResult<LeakageBudget> {
        Ok(LeakageBudget::zero())
    }

    async fn check_leakage_budget(
        &self,
        _context_id: ContextId,
        _observer: ObserverClass,
        _amount: u64,
    ) -> AuraResult<bool> {
        Ok(true)
    }

    async fn get_leakage_history(
        &self,
        _context_id: ContextId,
        _since_timestamp: Option<&PhysicalTime>,
    ) -> AuraResult<Vec<LeakageEvent>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn guard_chain_denies_transport_commands() {
    let authority = AuthorityId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);
    let context = ContextId::new_from_entropy([3u8; 32]);
    let effects = TestEffects::new(authority);

    let request = GuardRequest::new(authority, "amp:send".to_string(), 1)
        .with_context_id(context)
        .with_peer(peer)
        .with_context(context.to_bytes().to_vec());

    let plan = GuardPlan::new(
        request,
        vec![EffectCommand::SendEnvelope {
            to: aura_core::effects::NetworkAddress::from("peer"),
            peer_id: None,
            envelope: vec![1, 2, 3],
        }],
    );

    let send_count = Arc::new(AtomicUsize::new(0));
    let interpreter = Arc::new(CountingInterpreter {
        send_count: send_count.clone(),
    });

    let result = execute_guard_plan(&effects, &plan, interpreter)
        .await
        .unwrap();
    assert!(!result.authorized);
    assert_eq!(send_count.load(Ordering::SeqCst), 0);
}
