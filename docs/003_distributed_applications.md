# Aura Distributed Applications

This document demonstrates how Aura's theoretical foundations and system architecture come together in practice. It provides concrete examples of distributed systems, CRDT implementations, testing strategies, and integration patterns.

## Overview

This document shows working distributed systems built on Aura's foundations:

1. **Core Distributed Subsystems** - Search, garbage collection, and rendezvous protocols
2. **Concrete CRDT Examples** - GCounter, OR-Set, PN-Counter implementations
3. **Testing and Validation** - Strategies for testing distributed protocols
4. **System Integration** - How components work together with privacy guarantees

---

## 1. Core Distributed Subsystems

### 1.1 Distributed Search / Indexing

**Types:**
```rust
// Join layer
type InvIndex = Map<Term, DocSet>   // DocSet is a join-semilattice (e.g., OR-set)

// Meet layer
type Query    = Set<Term>           // constraints intersect

// Capability guards
cap need_post_index(term: Term)  // ability to publish term→docs for a context
cap need_query(term: Term)       // ability to ask for results on a term
```

**Global protocol (sketch):**
```
G_search:
  role A (querier), role N1..Nk (neighbors)
  state S0
  A -> Ni : QueryRequest{terms, nonce}     | need_query(terms)
  Ni: local_res := ⋂_{t∈terms} index[t]    // meet over doc sets
  Ni -> A : QueryReply{nonce, result_cids, proof} | need_post_index(terms)
  A: agg := ⋂ replies.result_cids
  A: optional fetch (capability-gated)
```

**Laws:**
- **Index correctness:** `index := index ⊔ δ` only grows; OR-set tombstones handled CRDT-safely
- **Privacy guard:** `QueryRequest` sent over **RID**/**GID** contexts; if broadcast via SBB, envelope padding + topic ratchets enforced to bound leakage
- **Capability meet:** A can only receive doc IDs for which `need_read(doc)` is satisfied under its context (responses filtered at Ni)
- **Flow budgets:** Each `QueryRequest`/`QueryReply` charges the `(RID, peer)` flow ledger before transport sends via the shared `FlowGuard` described in `docs/103_info_flow_budget.md`. New relationships therefore begin with low query throughput until trust raises their per-edge limit.

### 1.2 Distributed Garbage Collection (GC) & Snapshots

**Types:**
```rust
type SnapId
struct Snapshot { 
    root_commit: Hash, 
    watermarks: Map<Shard, EventId>, 
    proof: ThresholdSig 
}
cap need_snapshot_propose
cap need_snapshot_approve
cap need_prune_under(SnapId)
```

**Global protocol:**
```
G_gc:
  role P (proposer), role Q1..Qm (quorum)
  S0:
    P -> Qi : SnapProposal{candidate_root, cut} | need_snapshot_propose
    Qi: verify cut safe (local invariants)
    Qi -> P : SnapApprove{sig} | need_snapshot_approve
    P: combine FROST sigs -> Snapshot
    P -> all : SnapCommit{Snapshot}
    all: Journal.facts := compact(Journal.facts, Snapshot)
         Journal.facts ⊔= {GCMetadata(Snapshot)}
```

**Laws:**
- **Safety:** compaction is a *join-preserving retraction*; post-state is behaviorally equivalent for any future merge (CRDT homomorphism)
- **Privacy:** snapshot metadata carries no linkable identity beyond quorum class; messages sent under DKD app context; padding/batching per budget
- **Upgrade safety:** MPST version negotiation on `SnapProposal/Commit`; old clients can refuse prune but keep merging
- **Flow budgets:** Each gossip round invokes the shared `FlowGuard` before transporting `SnapProposal`, `SnapApprove`, or `SnapCommit`, ensuring spam and leakage controls match the definition in `docs/103_info_flow_budget.md`.

### 1.3 Rendezvous / Social Bulletin Board (SBB)

**Types:**
```rust
struct Envelope { cid: Cid, body: Cipher, pad_len: u16 }
cap can_post_envelope(topic)
cap can_fetch_topic(topic)
```

**Global protocol:**
```
G_rendezvous:
  roles: A (sender), R (relay neighbors), B (receiver)
  A: rt := rotate(K_tag, epoch)        // unlinkable routing tag
  A: env := aead_seal(K_box, payload)  // offer/answer/query/…
  A -> R* : SbbFlood{rt, Envelope} | can_post_envelope(topic(rt))
  R*: relay using HyParView/Plumtree (eager/lazy) with caps-checked rate limits
  B: check rt ∈ Valid(K_tag); aead_open(K_box, Envelope); process
```

**Laws:**
- **Delivery liveness:** Plumtree + HyParView ensure eventual reception in connected components
- **Privacy:** rotating tags + padding + cover traffic keep `adv_ngh` inference ≤ threshold
- **Authorization:** relays only forward if `can_post_envelope(topic)` holds after meet with relay policy; otherwise drop/greylist (rate-cap)
- **Flow budgets:** Relays require a signed `FlowReceipt` from the previous hop and charge their own ledger entry via `FlowGuard` before forwarding. When either hop runs out of budget, the envelope stalls without leaking timing through unauthorized contexts.

---

## 2. Concrete CRDT Examples

### 2.1 GCounter (Grow-only Counter)

A GCounter is a state-based counter that increases through componentwise maxima across replicas. It guarantees monotonic growth and simple convergence under joins. (CvRDT)

```rust
type Replica = String;
type Ctr = std::collections::BTreeMap<Replica, i64>;

impl JoinSemilattice for Ctr { 
    fn join(&self, o: &Self) -> Self { 
        pointwise_max(self, o) 
    } 
}

impl Bottom for Ctr { 
    fn bottom() -> Self { 
        BTreeMap::new() 
    } 
}

// Session (local): CvSync<Ctr> := μX. !Ctr. ?Ctr. X
// Handler: on recv c' => state = pointwise_max(state, c')
```

**Usage Example:**
```rust
use aura_protocol::effects::semilattice::{CvHandler, HandlerFactory};

let mut counter_handler = CvHandler::<Ctr>::new();
let session_id = SessionId::new();
let peers = replica_set();

HandlerFactory::execute_cv_sync(&mut counter_handler, peers, session_id).await?;
```

### 2.2 OR-Set (Observed-Remove Set)

An OR-Set tracks additions and removals using unique operation identifiers. Elements are present when added identifiers are not covered by corresponding removals. (CmRDT)

```rust
type OpId = (Replica, u64);

#[derive(Clone)] 
enum Op { 
    Add { elem: String, id: OpId }, 
    Rem { elem: String, tomb: std::collections::BTreeSet<OpId> } 
}

#[derive(Clone)] 
struct VV(std::collections::BTreeMap<Replica, u64>);

impl CausalOp for (Op, VV) { 
    type Id = OpId; 
    type Ctx = VV; 
    
    fn id(&self) -> Self::Id { 
        match &self.0 { 
            Op::Add { id, .. } => id.clone(), 
            Op::Rem { .. } => /* derive */ unimplemented!() 
        } 
    } 
    
    fn ctx(&self) -> &Self::Ctx { 
        &self.1 
    } 
}

// Session (local): OpBroadcast<OpWithCtx<(Op, VV), VV>>
```

**Usage Example:**
```rust
use aura_protocol::effects::semilattice::{CmHandler, HandlerFactory};

let mut orset_handler = CmHandler::<OrSetState, (Op, VV), OpId, VV>::new();
let session_id = SessionId::new();
let peers = replica_set();

HandlerFactory::execute_op_broadcast(&mut orset_handler, peers, session_id).await?;
```

### 2.3 PN-Counter (Positive-Negative Counter)

A PN-Counter maintains separate positive and negative components to support both increment and decrement. Its deltas are joined by pairwise addition to produce consistent totals. (Δ-CRDT)

```rust
type DeltaCtr = std::collections::BTreeMap<Replica, i64>; // component deltas

impl Delta for DeltaCtr { 
    fn join_delta(&self, o: &Self) -> Self { 
        pointwise_add(self, o) 
    } 
}

// Session (local): DeltaSync<DeltaCtr>
```

**Usage Example:**
```rust
use aura_protocol::effects::semilattice::{DeltaHandler, HandlerFactory};

let mut pncounter_handler = DeltaHandler::<PNCounterState, DeltaCtr>::new();
let session_id = SessionId::new();
let peers = replica_set();

HandlerFactory::execute_delta_gossip(&mut pncounter_handler, peers, session_id).await?;
```

### 2.4 Implementation Steps (Complete CRDT System)

1. **Define the CRDT traits** (`JoinSemilattice`, `CvState`, `Delta`, `CmApply`, `Dedup`, `CausalOp`)
2. **Add message wrappers** (`StateMsg`, `DeltaMsg`, `OpWithCtx`) and concrete payload types
3. **Implement delivery/order effects** (`CausalBroadcast`, `AtLeastOnce`, `GossipTick`, `ExchangeDigest`)
4. **Provide generic handlers** (Cv/Δ/Cm) binding session events to CRDT laws
5. **Ship session templates**: `CvSync<S>`, `DeltaSync<Δ>`, `OpBroadcast<Op, Ctx>`, `OpRepair` as reusable Aura protocols
6. **Tooling to pretty-print global protocols** and **projected locals** for audits

---

## 3. Testing and Validation Strategies

### 3.1 Unit Testing Patterns

**Effect-Based Unit Tests:**
```rust
#[tokio::test]
async fn test_threshold_ceremony() {
    let crypto = MockCryptoHandler::new();
    let network = TestNetworkEffects::default();
    let storage = MemoryStorageHandler::new();

    let result = run_threshold_ceremony(&crypto, &network, &storage).await?;
    assert_eq!(result.participants.len(), 3);
    assert_eq!(result.threshold, 2);
}
```

**CRDT Property Tests:**
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn crdt_associativity(a: GCounter, b: GCounter, c: GCounter) {
        let left = a.join(&b).join(&c);
        let right = a.join(&b.join(&c));
        prop_assert_eq!(left, right);
    }
    
    #[test]
    fn crdt_commutativity(a: GCounter, b: GCounter) {
        let left = a.join(&b);
        let right = b.join(&a);
        prop_assert_eq!(left, right);
    }
    
    #[test]
    fn crdt_idempotence(a: GCounter) {
        let result = a.join(&a);
        prop_assert_eq!(result, a);
    }
}
```

### 3.2 Integration Testing

**Multi-Node Protocol Tests:**
```rust
#[tokio::test]
async fn test_distributed_search() {
    let simulator = NetworkSimulator::new()
        .with_nodes(5)
        .with_search_indices();
        
    // Node 1 publishes documents
    simulator.node(1).publish_document("doc1", "content").await?;
    
    // Node 3 searches
    let results = simulator.node(3).search_query("content").await?;
    
    assert!(results.contains("doc1"));
    assert_eq!(results.len(), 1);
}
```

**CRDT Convergence Tests:**
```rust
#[tokio::test]
async fn test_orset_convergence() {
    let simulator = CrdtSimulator::new()
        .with_replicas(3)
        .with_network_partitions();
        
    // Operations during partition
    simulator.partition_network();
    simulator.replica(0).add("item1").await?;
    simulator.replica(1).add("item2").await?;
    simulator.replica(2).remove("item1").await?;
    
    // Heal partition and verify convergence
    simulator.heal_network().await;
    simulator.synchronize_all().await;
    
    let final_state = simulator.replica(0).get_state();
    assert!(!final_state.contains("item1")); // Removed
    assert!(final_state.contains("item2"));  // Added
    
    // All replicas converged to same state
    for i in 1..3 {
        assert_eq!(final_state, simulator.replica(i).get_state());
    }
}
```

**Guardian Recovery + Invitation Smoke Tests:**

- `cargo test -p aura-recovery guardian_recovery` exercises `crates/aura-recovery/tests/guardian_recovery.rs`, covering the FlowGuard-backed happy path plus cooldown enforcement by reusing the shared guardian ledger defined in `choreography_impl.rs`.
- `cargo test -p aura-invitation invitation_flow` drives `DeviceInvitationCoordinator` and `InvitationAcceptanceCoordinator` end-to-end, validating content-addressed envelopes, ledger updates, and FlowGuard hints before broadcasts.
- CLI wrappers (`aura recovery start`, `aura invite create|accept`) are thin shells over the same coordinators, so running those commands after `just build` gives operator confidence that the live flows match the test harnesses.

### 3.3 Property-Based Testing for Protocols

**Deterministic Protocol Testing:**
```rust
proptest! {
    #[test]
    fn prop_dkd_deterministic(seed: u64, participants: Vec<DeviceId>) {
        prop_assume!(participants.len() >= 2 && participants.len() <= 10);
        
        let mut simulator = DeterministicSimulator::with_seed(seed);
        let results: Vec<DkdResult> = participants.iter()
            .map(|&device_id| {
                let effects = simulator.create_effects(device_id);
                tokio_test::block_on(async {
                    run_dkd_protocol(&effects, &participants).await
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
            
        // All participants derive the same key
        let derived_keys: Vec<_> = results.iter().map(|r| &r.derived_key).collect();
        for key in derived_keys.windows(2) {
            prop_assert_eq!(key[0], key[1]);
        }
    }
}
```

### 3.4 Chaos Testing

**Byzantine Fault Injection:**
```rust
#[tokio::test]
async fn test_byzantine_resilience() {
    let simulator = ByzantineSimulator::new()
        .with_honest_nodes(4)
        .with_byzantine_nodes(1)
        .with_fault_types(vec![
            FaultType::MessageDrop(0.1),
            FaultType::MessageDelay(Duration::from_secs(2)),
            FaultType::EquivocatingBroadcast,
        ]);
        
    let result = simulator.run_consensus_protocol().await?;
    
    // Safety: Byzantine nodes cannot break agreement
    assert!(result.all_honest_agree());
    
    // Liveness: Protocol terminates despite faults
    assert!(result.terminated_within(Duration::from_secs(30)));
}
```

### 3.5 Privacy Testing

**Context Isolation Verification:**
```rust
#[tokio::test]
async fn test_context_isolation() {
    let observer = PrivacyObserver::new();
    let simulator = PrivacySimulator::new()
        .with_observer(observer)
        .with_contexts(vec![
            RelationshipContext::new(alice, bob),
            RelationshipContext::new(alice, charlie),
        ]);
        
    // Run protocol with different contexts
    simulator.alice().send_to_bob("message1", context_ab).await?;
    simulator.alice().send_to_charlie("message2", context_ac).await?;
    
    let trace = simulator.observer().get_trace();
    
    // Observer cannot distinguish which context was used
    assert!(trace.contexts_are_unlinkable());
    
    // No cross-context message leakage
    assert!(trace.verify_context_isolation());
}
```

**Leakage Budget Enforcement:**
```rust
#[tokio::test]
async fn test_leakage_budgets() {
    let budget_tracker = LeakageBudgetTracker::new()
        .with_external_budget(1.0)
        .with_neighbor_budget(0.5)
        .with_group_budget(0.1);
        
    let handler = AuraEffectSystem::for_testing(device_id)
        .with_privacy_budgets(budget_tracker);
        
    // Operations that would exceed budget should be rejected
    let result = handler.broadcast_message(msg, LeakageClass::External).await;
    if budget_tracker.would_exceed_budget(LeakageClass::External, msg.leakage()) {
        assert!(result.is_err());
    } else {
        assert!(result.is_ok());
    }
}
```

### 3.6 Verification Matrix

Link core invariants from `docs/001_theoretical_foundations.md` to tests in this document:
- Charge‑Before‑Send / No‑Observable‑Without‑Charge → Transport tests that assert no packet emission on budget or cap denial.
- Deterministic‑Replenishment → Epoch rotation/property tests that demonstrate meet/join merge behavior and convergence.
- Meet‑before‑Join discipline → Session tests that only commit attested facts; no rollback/negative facts compile.
- Convergence Bound → Multi‑replica tests showing `spent(ctx) ≤ min_r limit_r(ctx)` after convergence within an epoch.

---

## 4. System Integration Patterns

### 4.1 Putting It All Together (Glue Invariants)

**1. Monotone Convergence:**
- Facts grow by ⊔; Caps shrink by ⊓
- All protocols preserve `facts' ≥ facts` and `caps' ≤ caps`

**2. Guarded Progress:**
- MPST progress holds *provided guards discharge*: if a role lacks caps, the protocol does not deadlock — it **refuses earlier** (typestate cannot advance), which is observable but bounded by leakage budgets (dummy/cover transitions available)

**3. Context-Bound Privacy:**
- Every message inhabits exactly one context (`RID`, `GID`, or DKD)
- Cross-context flow must go through an explicit **bridge protocol** with its own guards and leakage accounting

**4. Capability Soundness:**
- For any emitted side-effect `σ` (send, store, prune), there exists a proof term/witness `w` such that `need(σ) ≤ caps(ctx)` at the sender role. Handlers enforce this mechanically.

**5. CRDT/GC Compatibility:**
- Snapshot/GC is a semilattice homomorphism `h : Fact → Fact` with `h(x ⊔ y) = h(x) ⊔ h(y)` and `h(x) ≤ x`
- Therefore compaction commutes with future merges

**6. Search Correctness under Privacy:**
- Returned results are `⋂_i local_i(terms)`, but each `local_i` is **cap-filtered**: no doc identifier escapes without `need_read(doc)` under the active context. Aggregation therefore cannot leak superset IDs.

### 4.2 Complete Application Example

**Threshold Identity with Social Recovery:**
```rust
use aura_agent::AuraAgent;
use aura_protocol::effects::system::AuraEffectSystem;
use aura_recovery::guardian_recovery::{
    GuardianRecoveryCoordinator, GuardianRecoveryRequest, RecoveryPriority, RecoveryOperationType,
};
use aura_authenticate::RecoveryContext;
use aura_wot::Guardian;
use aura_core::{AccountId, Cap, DeviceId, Journal};
use aura_mpst::AuraRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device_id = DeviceId::new();
    let effects = AuraEffectSystem::for_production(device_id)?;
    let mut agent = AuraAgent::new(effects, device_id);
    agent.initialize().await?;

    // Kick off guardian recovery choreography from the recovery crate
    let runtime = aura_mpst::AuraRuntime::new(device_id, Cap::top(), Journal::new());
    let mut coordinator = GuardianRecoveryCoordinator::new(runtime);
    let guardian = Guardian::new(DeviceId::new(), "Alice".into());
    let request = GuardianRecoveryRequest {
        requesting_device: device_id,
        account_id: AccountId::new(),
        recovery_context: RecoveryContext {
            operation_type: RecoveryOperationType::DeviceKeyRecovery,
            justification: "Lost primary device".into(),
            is_emergency: false,
            timestamp: 0,
        },
        required_threshold: 2,
        available_guardians: vec![guardian],
        priority: RecoveryPriority::Normal,
        dispute_window_secs: 48 * 60 * 60,
    };

    coordinator.execute_recovery(request).await?;
    Ok(())
}
```

### 4.3 Import Patterns

```rust
// Currently available types (aura-core)
use aura_core::session_epochs::{ParticipantId, SessionStatus, LocalSessionType};
use aura_core::protocols::{ProtocolType, ThresholdConfig, ProtocolSessionStatus};
use aura_core::identifiers::{SessionId, EventId, DeviceId};

// Current effect system (aura-protocol)
use aura_protocol::effects::system::AuraEffectSystem;
use aura_protocol::effects::{CryptoEffects, NetworkEffects, StorageEffects};

// CRDT system integration
use aura_core::semilattice::{StateMsg, CvState, MeetStateMsg, MvState};
use aura_protocol::effects::semilattice::{
    HandlerFactory, CvHandler, MvHandler, DeltaHandler, CmHandler,
};
use aura_journal::semilattice::{JournalMap, CapabilitySet, ModernAccountState};

// Future choreographic integration (when implemented)
// use aura_core::sessions::{Protocol, Label, Branch};
// use aura_core::effects::choreographic::{Effect, Program};
// use rumpsteak_choreography::choreography;
```

### 4.4 Runtime Integration Pattern

```rust
use aura_protocol::choreography::protocols::threshold_ceremony::{
    execute_threshold_ceremony, CeremonyConfig, CeremonyResult, CeremonyError,
};
use aura_protocol::effects::system::AuraEffectSystem;

pub struct CeremonyRole {
    pub is_coordinator: bool,
    pub is_observer: bool,
    pub signer_index: Option<usize>,
}

pub async fn run_ceremony(
    device_id: DeviceId,
    role: CeremonyRole,
    config: CeremonyConfig,
    effects: &AuraEffectSystem,
) -> Result<CeremonyResult, CeremonyError> {
    execute_threshold_ceremony(
        device_id,
        config,
        role.is_coordinator,
        role.is_observer,
        role.signer_index,
        effects,
    ).await
}
```

---

## 5. Privacy Contract Implementation

### 5.1 Communication Rule Set

**1. Replicated truths ⇒ Semilattices**
Anything you expect to survive partitions and be merged across devices/peers should be a lattice:
- **Facts/knowledge:** join (⊔) CRDTs in the journal
- **Authority/scope:** meet (⊓) capabilities (computed at use-time, not stored as growing "facts")
- **Availability states:** use small lattices like `{none < cached < pinned}`

**2. Interactive control flow ⇒ MPST sessions (not lattices), but "monotonize the output"**
Rounds of a threshold ceremony, a recovery vote, a rendezvous offer/answer, or a search RPC are **not** themselves CRDTs. Run them as multi-party session types, and ensure their **projection to the journal** is monotone:
- The session either emits a **new fact** (e.g., `TreeOp` with threshold sig) or nothing
- Avoid negative facts/rollbacks; use epochs and replacement-by-join (e.g., "latest attested config")

**3. Two-phase monotonicity (CALM-style): meets before, joins after**
- Preconditions as **meets**: guards/caps/policies only **shrink** what's allowed
- Commit as a **join**: once a result is attested, the journal **only grows** with that fact

**4. Model lifecycles as increasing lattices, not deletes**
For intents/proposals, use a small partial order (e.g., `Proposed < Attesting < Finalized | Aborted`) with an LWW tie-breaker, so merges remain monotone while still expressing "failed/aborted".

**5. Keep non-replicated streams out of the lattice**
Telemetry, flow control, chunk transfer windows, backpressure signals—these are ephemeral; don't shoehorn them into a CRDT. Let them live in transport/session logic; only publish **durable milestones** to the journal.

**6. Search is split by design**
- Index state = join CRDT (term → OR-set(doc))
- Queries = meet over that state (constraint refinement)
- Only cache/index updates (plus optional signed attestations) should land in the journal; query/response traffic stays in the session layer.

**7. Rendezvous/SBB stay session-scoped**
- Flooding, rotating tags, retries, and backoff live entirely in the choreographies
- Journals only record envelopes-as-facts and relationship/device state changes

**8. GC/Snapshots must be join-preserving retractions**
- The session to reach a cut is typed; the snapshot that lands in the journal is a retraction `h` with `h(x ⊔ y) = h(x) ⊔ h(y)` and `h(x) ≤ x`
- This ensures compaction commutes with future merges

### 5.2 Decision Test

- "Will this cross partitions and need to merge?" → put its **result** in a lattice
- "Is this transient control/negotiation?" → keep it in MPST sessions; **only the commit** becomes a lattice fact
- "Does it remove information?" → redesign as epoch/replace or a retraction proof that preserves joins

So: semilattices are the **boundary discipline** for durable, mergeable state; MPST sessions are the **engine** for rich multi-party coordination. Use both, and glue them with the pattern: **meet-guarded preconditions, join-only commits.**

Flow budgets live right beside these facts: every `(context, peer)` pair stores `FlowBudget { limit, spent, epoch }` in the journal. Because `spent` is a join (max) and `limit` is a meet (min), budgeting fits seamlessly into this decision tree—rate limiting spam and bounding metadata leak both boil down to guarding effect calls with the same monotone data. See `docs/004_info_flow_model.md` for receipt structure, epoch rotation, fairness, and liveness guidance.

---

## 6. Performance and Optimization

### 6.1 Hot Path Optimization

For performance-critical choreographies, use typed traits directly:

```rust
// Hot path: zero overhead typed dispatch
async fn dkd_commitment<C: CryptoEffects, R: RandomEffects>(
    crypto: &C,
    random: &R,
) -> Commitment {
    let nonce = random.random_bytes(32).await;  // FAST: Direct call, fully inlined
    let hash = crypto.blake3_hash(&data).await;  // FAST: Zero overhead
    Commitment { hash, nonce }
}

// Call with concrete handler - zero overhead
let handler = AuraEffectSystem::for_testing(device_id);
let commitment = dkd_commitment(&handler, &handler).await;
```

### 6.2 Dynamic Composition for Flexibility

For middleware stacking and runtime composition:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

// Middleware stacking uses type-erased handlers wrapped in Arc<RwLock<>>
fn create_handler(config: &Config) -> Arc<RwLock<Box<dyn AuraHandler>>> {
    let base = AuraEffectSystem::for_testing(device_id);
    let with_retry = RetryMiddleware::new(base, 3);
    let with_tracing = TracingMiddleware::new(with_retry, "svc");
    Arc::new(RwLock::new(Box::new(with_tracing)))
}

// Can STILL use typed traits through blanket impl!
let handler = create_handler(&config);
let bytes = handler.random_bytes(32).await;  // Works! Uses blanket impl
```

### 6.3 When to Use Which Pattern

**Use Typed Traits When:**
- Hot loops with millions of iterations
- Performance-critical choreographies (DKD, FROST)
- Known concrete types at compile time
- Want maximum compiler optimization
- Writing unit tests with typed mocks

**Use Type-Erased When:**
- Dynamic handler selection at runtime
- Building middleware stacks
- Heterogeneous collections of handlers
- Plugin systems or dynamic loading
- Simpler function signatures (avoid generic soup)

---

## 7. Deployment and Operations

### 7.1 Cross-Platform Considerations

Aura runs on multiple platforms from day one:

**Web (WebAssembly):**
```rust
#[cfg(target_arch = "wasm32")]
pub fn create_web_handler(device_id: DeviceId) -> Result<AuraEffectSystem, AuraError> {
    AuraEffectSystem::for_production(device_id)
        .with_storage(WebStorageHandler::new())
        .with_network(WebSocketNetworkHandler::new())
        .with_crypto(WasmCryptoHandler::new())
        .build()
}
```

**Mobile (iOS/Android):**
```rust
#[cfg(target_os = "ios")]
pub fn create_ios_handler(device_id: DeviceId) -> Result<AuraEffectSystem, AuraError> {
    AuraEffectSystem::for_production(device_id)
        .with_storage(KeychainStorageHandler::new())
        .with_network(UrlSessionNetworkHandler::new())
        .with_crypto(SecurityFrameworkCryptoHandler::new())
        .build()
}
```

**Desktop (Native):**
```rust
#[cfg(not(any(target_arch = "wasm32", target_os = "ios", target_os = "android")))]
pub fn create_desktop_handler(device_id: DeviceId) -> Result<AuraEffectSystem, AuraError> {
    AuraEffectSystem::for_production(device_id)
        .with_storage(FilesystemStorageHandler::new())
        .with_network(TcpNetworkHandler::new())
        .with_crypto(NativeCryptoHandler::new())
        .build()
}
```

### 7.2 Monitoring and Observability

```rust
// Production deployment with full observability
let handler = AuraEffectSystem::for_production(device_id)?
    .with_middleware(MetricsMiddleware::new("aura-node"))
    .with_middleware(TracingMiddleware::new("distributed", trace_config))
    .with_middleware(HealthCheckMiddleware::new("/health"))
    .build();

// Emit structured logs for distributed tracing
handler.emit_event("protocol.dkd.started", json!({
    "participants": participant_count,
    "threshold": threshold,
    "context": context_id
})).await;
```

## Conclusion

Aura's distributed applications demonstrate that the theoretical foundations translate into practical, working systems. The combination of:

- **Algebraic effect handlers** for composable system architecture
- **Session-typed choreographies** for safe distributed coordination  
- **Semilattice CRDTs** for conflict-free state replication
- **Privacy-preserving contexts** for secure communication

...enables building complex distributed applications while maintaining strong safety, privacy, and correctness guarantees.

The examples in this document provide concrete templates for implementing distributed systems on Aura's foundations, from simple CRDT replication to complex multi-party protocols with threshold cryptography and social recovery.

## See Also

- `001_theoretical_foundations.md` - Mathematical foundations and formal model
- `002_system_architecture.md` - Implementation patterns and system design  
- `000_overview.md` - Overall project architecture and goals
