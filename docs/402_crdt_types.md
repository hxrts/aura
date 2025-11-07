# Expressing CRDTs with Aura Session Types & Effects

Aura represents CRDT replication protocols using multi party session types. This document shows how conflict-free replicated data types (CRDTs) can be expressed by complementing our session type algebra with type information for state joins, causal context, and delivery ordering beyond what the session algebra alone provides. The text defines the type structure, message forms, and handler semantics that make this possible. It uses the same notation as the Aura session type system and effect algebra.

---

## 0. Foundations & Notation (Aura)

### 0.1 Global MPST (roles explicit)

```
G ::= r₁ → r₂ : T . G            |   // point-to-point
      r  → *  : T . G            |   // broadcast (one-to-many)
      G ∥ G                      |   // parallel composition
      r ⊳ { ℓᵢ : Gᵢ }ᵢ∈I        |   // role r selects a labeled branch
      μX . G                    |   // recursion
      X                         |   // recursion variable
      end                           // termination

T ::= Unit | Bool | Int | String | …   // message types
r ::= role identifiers (Alice, Bob, …)
ℓ ::= label identifiers (accept, reject, …)
```
```rust
enum Protocol {
    Send { from: Role, to: Role, msg: MessageType, continuation: Box<Protocol> },
    Broadcast { from: Role, to_all: Vec<Role>, msg: MessageType, continuation: Box<Protocol> },
    Parallel { protocols: Vec<Protocol> },
    Choice { role: Role, branches: Vec<(Label, Protocol)> },
    Rec { label: String, body: Box<Protocol> },
    Var(String),
    End,
}
```

### 0.2 Local projection (per role)

```
L ::= ! T . L
    | ? T . L
    | ⊕ { ℓᵢ : Lᵢ }ᵢ∈I
    | & { ℓᵢ : Lᵢ }ᵢ∈I
    | μX . L | X | end
```
```rust
// Standard binary local grammar after projection
L ::= !T.L | ?T.L | ⊕{ℓi : L} | &{ℓi : L} | End | μX.L | X
```

### 0.3 Effects layer (Aura)

```rust
enum Effect<R, M> {
    Send { to: R, msg: M }, Recv { from: R, msg_type: &'static str },
    Choose { at: R, label: Label }, Offer { from: R },
    Branch { choosing_role: R, branches: Vec<(Label, Program<R, M>)> },
    Parallel { programs: Vec<Program<R, M>> },
    Loop { iterations: Option<usize>, body: Program<R, M> },
    Timeout { role: R, duration: Duration, body: Program<R, M> },
    End,
}

type Program<R, M> = Vec<Effect<R, M>>; // sequential composition
```

---

## 1. CRDT Semantic Interfaces (Types)

We model CRDT laws via traits that handlers enforce. These are orthogonal to session typing and used to type message payloads.

```rust
// State-based (CvRDT)
pub trait JoinSemilattice: Clone { fn join(&self, other: &Self) -> Self; }
pub trait Bottom { fn bottom() -> Self; }
pub trait CvState: JoinSemilattice + Bottom {}

// Delta CRDTs
pub trait Delta: Clone { fn join_delta(&self, other: &Self) -> Self; }
pub trait DeltaProduce<S> { fn delta_from(old: &S, new: &S) -> Self; }

// Op-based (CmRDT)
pub trait CausalOp { type Id: Clone; type Ctx: Clone; fn id(&self) -> Self::Id; fn ctx(&self) -> &Self::Ctx; }
pub trait CmApply<Op> { fn apply(&mut self, op: Op); } // commutes under causal delivery
pub trait Dedup<I> { fn seen(&self, id: &I) -> bool; fn mark_seen(&mut self, id: I); }
```

**Delivery assumptions** are provided by **effects** (Section 4), not by session grammar.

---

## 2. Message Types (precise `T` carried by sessions)

```rust
// Phantom tags for clarity (optional at runtime)
#[derive(Clone)] pub enum MsgKind { FullState, Delta, Op }

pub type StateMsg<S> = (S, MsgKind);      // (payload, tag::FullState)
pub type DeltaMsg<D> = (D, MsgKind);      // (payload, tag::Delta)

#[derive(Clone)] pub struct OpWithCtx<Op, Ctx> { pub op: Op, pub ctx: Ctx }

// Auxiliary protocol payloads
pub type Digest<Id> = Vec<Id>;
pub type Missing<Op> = Vec<Op>;
```

These are ordinary `T` in local session types; handlers add semantics via traits.

---

## 3. Protocol Schemas in Aura

We provide **global** schemas with roles, then show **local** projections (binary grammar). All CRDT families fit by instantiating `T`.

### 3.1 CvRDT (State-based Anti-Entropy)

CvRDTs (Convergent Replicated Data Types) synchronize by state exchange.
Each replica periodically sends its full state to others, who merge it using the join operation from a semilattice. Merging always moves toward convergence regardless of order or duplication

This process is known as anti-entropy: it continuously reduces divergence by ensuring that all replicas monotonically approach the least upper bound of their states.

Aura’s session type for CvRDTs captures anti-entropy as a symmetric, recursive send/receive loop, which models bidirectional anti-entropy between roles A and B.

**Global (roles A,B):**
```
CvSync<S> := μX . (A → B : State<S> . X) ∥ (B → A : State<S> . X)
```
```rust
Rec "loop": Parallel {
  protocols: vec![
    Send { from: A, to: B, msg: State<S>, continuation: Var("loop").into() },
    Send { from: B, to: A, msg: State<S>, continuation: Var("loop").into() },
  ]
}
```

**Local (A and B are symmetric):**

```
CvSync<S> := μX . ! StateMsg<S> . ? StateMsg<S> . X
```

**Handler law:** on receive `s'`: `state := state.join(&s')`.

**Variants**

* Push-only: `μX . ! StateMsg<S> . X`
* Pull-only: `μX . ? StateMsg<S> . X`
* Periodic full repair: interleave full `S` after `k` deltas.

### 3.2 Δ-CRDT (Delta-based Gossip)

Δ-CRDTs optimize CvRDTs by transmitting deltas rather than full states (reducing bandwidth). These deltas are disseminated through a gossip protocol, where replicas periodically share recent updates with peers in a decentralized, probabilistic manner. Each delta represents a join fragment that can be combined into the full state through local accumulation and periodic folding.

In Aura, this corresponds to the same recursive structure but over DeltaMsg<Δ> messages. The gossip layer (in the effect algebra) provides eventual delivery guarantees, ensuring all deltas reach every replica in some order.

**Local (symmetric):**

```
DeltaSync<Δ> := μX . ! DeltaMsg<Δ> . ? DeltaMsg<Δ> . X
```

**Handler law:** buffer/accumulate `Δ` then fold into `S` periodically: `S := S ⊔ fold(Δ*)`.

### 3.3 CmRDT (Operation-based)

CmRDTs (Commutative Replicated Data Types) propagate operations instead of states or deltas. To maintain correctness, they depend on causal broadcast, a network abstraction that guarantees all operations are delivered to every replica in causal order.

Each operation carries a causal context (like a vector clock or dependency set) so that applying them commutes across replicas. The associated CausalBroadcast delivery effect enforces the happens-before relationship, ensuring consistent convergence.

**Global (N replicas r ∈ Replicas):**

```
OpBroadcast<Op, Ctx> := μX . ( r ⊳ {
  issue : r → * : OpWithCtx<Op, Ctx> . X,
  idle  : end
} )
```
```rust
Rec "loop": Choice { role: Ri, branches: vec![
  ("issue", Broadcast { from: Ri, to_all: Replicas \ {Ri}, msg: OpWithCtx<Op, Ctx>, continuation: Var("loop").into() }),
  ("idle", End),
]}
```

**Local (issuer at r):** `μX . ⊕{ issue : ! OpWithCtx<Op, Ctx> . X, idle : end }`

**Local (subscriber at r' ≠ r):** `μX . &{ issue : ? OpWithCtx<Op, Ctx> . X, idle : end }`

**Handler law:** deliver in causal order (or buffer until ready), dedup by `id`, then `state.apply(op)`.

### 3.4 Repair for CmRDT

Repair protocols exchange missing operation digests to recover from message loss or gaps in causal history. They maintain convergence by ensuring that all replicas eventually apply the same set of operations. for CmRDT

**Global (pairwise repair between A,B):**

```
OpRepair<Id, Op> := μX . A ⊳ {
  pull : A → B : Digest<Id> . B → A : Missing<Op> . X,
  idle : end
}
```
```rust
Rec "loop": Choice { role: A, branches: vec![
  ("pull", Send{ from: A, to: B, msg: Digest<OpId>, continuation:
       Receive{ from: B, to: A, msg: Missing<Op>, continuation: Var("loop").into() }.into() }),
  ("idle", End),
]}
```

**Local (A):** `μX . ⊕{ pull : ! Digest<Id> . ? Missing<Op> . X, idle : end }`

**Local (B):** `μX . &{ pull : ? Digest<Id> . ! Missing<Op> . X, idle : end }`

---

## 4. Effects for Delivery & Ordering (Aura)

We realize CRDT semantics by composing **session effects** with **delivery/order effects**.

```rust
// Delivery/order effects used alongside SessionSend/Recv
pub enum DeliveryEffect {
    CausalBroadcast { topic: TopicId },  // ensures happens-before delivery
    AtLeastOnce    { topic: TopicId },   // retries; dedup in handler
    GossipTick     { topic: TopicId },   // drive periodic exchange
    ExchangeDigest,                      // trigger repair subprotocol
}
```

**Programs** stitch them with session ops:

```rust
let prog: Program<Role, Message> = vec![
  Effect::Choose { at: Ri, label: "issue" },
  Effect::Send { to: Rj, msg: Message::Op(op_with_ctx) },
  Effect::Parallel { programs: vec![ /* concurrent sends to peers */ ] },
  Effect::End,
];
```

---

## 5. Generic Handlers (Enforcing CRDT Laws)

### 5.1 CvRDT

```rust
pub struct CvHandler<S: CvState> { pub state: S }
impl<S: CvState> CvHandler<S> {
    pub fn on_recv(&mut self, msg: StateMsg<S>) { self.state = self.state.join(&msg.0); }
}
```

### 5.2 Δ-CRDT

```rust
pub struct DeltaHandler<S: CvState, D: Delta> { pub state: S, pub inbox: Vec<D> }
impl<S: CvState, D: Delta> DeltaHandler<S, D> {
    pub fn on_recv(&mut self, d: DeltaMsg<D>) { self.inbox.push(d.0); }
    pub fn fold(&mut self) { let agg = self.inbox.drain(..).reduce(|a,b| a.join_delta(&b)).unwrap_or_else(|| /* empty Δ */); /* state := state ⊔ agg */ }
}
```

### 5.3 CmRDT

```rust
pub struct CmHandler<S, Op, Id, Ctx>
where S: CmApply<Op> + Dedup<Id>, Op: CausalOp<Id=Id, Ctx=Ctx> {
    pub state: S,
}
impl<S, Op, Id: Clone, Ctx> CmHandler<S, Op, Id, Ctx>
where S: CmApply<Op> + Dedup<Id>, Op: CausalOp<Id=Id, Ctx=Ctx> {
    pub fn on_recv(&mut self, m: OpWithCtx<Op, Ctx>) {
        if delivery::causal_ready(&m.ctx) && !self.state.seen(&m.op.id()) {
            self.state.apply(m.op);
            self.state.mark_seen(m.op.id());
        } else { buffer(m); }
    }
}
```

`delivery::causal_ready` can be implemented with version vectors or dependency sets encoded in `Ctx`.

---

## 6. Precise `T` Instantiations (Examples)

### 6.1 GCounter (Grow-only Counter)

A GCounter is a state-based counter that increases through componentwise maxima across replicas. It guarantees monotonic growth and simple convergence under joins. (CvRDT)

```rust
type Replica = String;
type Ctr = std::collections::BTreeMap<Replica, i64>;
impl JoinSemilattice for Ctr { fn join(&self, o:&Self)->Self { pointwise_max(self,o) } }
impl Bottom for Ctr { fn bottom()->Self { BTreeMap::new() } }

// Session (local): CvSync<Ctr> := μX. !Ctr. ?Ctr. X
// Handler: on recv c' => state = pointwise_max(state, c')
```

### 6.2 OR-Set (Observed-Remove Set)

An OR-Set tracks additions and removals using unique operation identifiers. Elements are present when added identifiers are not covered by corresponding removals. (CmRDT)

```rust
type OpId = (Replica, u64);
#[derive(Clone)] enum Op { Add{elem:String, id:OpId}, Rem{elem:String, tomb:std::collections::BTreeSet<OpId>} }
#[derive(Clone)] struct VV(std::collections::BTreeMap<Replica, u64>);
impl CausalOp for (Op, VV) { type Id = OpId; type Ctx = VV; fn id(&self)->Self::Id { match &self.0 { Op::Add{id,..} => id.clone(), Op::Rem{..} => /* derive */ unimplemented!() } } fn ctx(&self)->&Self::Ctx { &self.1 } }
// Session (local): OpBroadcast<OpWithCtx<(Op, VV), VV>>
```

### 6.3 PN-Counter (Positive-Negative Counter)

A PN-Counter maintains separate positive and negative components to support both increment and decrement. Its deltas are joined by pairwise addition to produce consistent totals. (Δ-CRDT)

```rust
type DeltaCtr = std::collections::BTreeMap<Replica, i64>; // component deltas
impl Delta for DeltaCtr { fn join_delta(&self, o:&Self)->Self { pointwise_add(self,o) } }
// Session (local): DeltaSync<DeltaCtr>
```

---

## 7. Aura Global ↔ Local Examples

**Global OR-Set broadcast (sketch):**

```
μX . ( r ⊳ {
  issue : r → * : OpWithCtx<Op, Ctx> . X,
  idle  : end
} )
```
```rust
Protocol::Rec { label: "loop", body: Box::new(Protocol::Choice {
  role: Role::Any, branches: vec![
    ("issue", Protocol::Broadcast { from: Role::Any, to_all: REPLICAS, msg: MessageType::OpWithCtx, continuation: Box::new(Protocol::Var("loop".into())) }),
    ("idle", Protocol::End),
  ]
})
```

Projection gives `issuer: μX . ⊕{ issue : ! OpWithCtx<Op, Ctx> . X, idle : end }`, `subscriber: μX . &{ issue : ? OpWithCtx<Op, Ctx> . X, idle : end }`.

---

## 8. Safety & Convergence Sketches

* **Session safety**: Projection ensures dual locals, communication safety, and deadlock freedom (given guarded recursion and conflict-free parallels).
* **Cv/Δ convergence**: eventual delivery + semilattice laws ⇒ states converge to the join of all local updates.
* **Cm convergence**: causal delivery + dedup + commutative (or effect-equivalent) ops ⇒ replicas converge modulo permutation of independent ops.

---

## 9. Implementation Steps (Aura)

1. Define the CRDT traits (`JoinSemilattice`, `CvState`, `Delta`, `CmApply`, `Dedup`, `CausalOp`).
2. Add message wrappers (`StateMsg`, `DeltaMsg`, `OpWithCtx`) and concrete payload types.
3. Implement **delivery/order effects** (`CausalBroadcast`, `AtLeastOnce`, `GossipTick`, `ExchangeDigest`).
4. Provide **generic handlers** (Cv/Δ/Cm) binding session events to CRDT laws.
5. Ship **session templates**: `CvSync<S>`, `DeltaSync<Δ>`, `OpBroadcast<Op, Ctx>`, `OpRepair` as reusable Aura protocols.
6. Tooling to pretty-print **global protocols** and **projected locals** for audits.

---

## 10. Harmonized Implementation Architecture

### 10.1 Separation of Concerns

Aura's CRDT system implements the session-type algebra through a **4-layer architecture** that separates:

1. **Semantic Foundation** - Core CRDT traits and message type definitions
2. **Effect Interpretation** - Composable handlers that enforce CRDT laws
3. **Choreographic Protocols** - Session-type communication patterns
4. **Application CRDTs** - Domain-specific implementations

This separation ensures that **typed messages** (`T` in session types) and **effect interpreters** (semantic law enforcement) are clearly defined and composable.

### 10.2 File Organization

```
aura-types/src/semilattice/          # Foundation layer (workspace-wide)
├── semantic_traits.rs               # JoinSemilattice, MeetSemiLattice, CvState, MvState, etc.
├── message_types.rs                 # StateMsg<S>, MeetStateMsg<S>, OpWithCtx<Op,Ctx>, etc.
├── tests/                          # Property-based tests for algebraic laws
│   └── meet_properties.rs          # Meet semi-lattice law validation
└── mod.rs                          # Re-exports and trait implementations

aura-protocol/src/effects/semilattice/  # Effect interpreter layer
├── cv_handler.rs                   # CvHandler<S: CvState> - join-based state CRDTs
├── mv_handler.rs                   # MvHandler<S: MvState> - meet-based constraint CRDTs
├── delta_handler.rs                # DeltaHandler<S,D> - delta-based  
├── cm_handler.rs                   # CmHandler<S,Op> - operation-based
├── delivery.rs                     # CausalBroadcast, AtLeastOnce effects
└── mod.rs                          # Handler composition utilities

aura-choreography/src/semilattice/  # Choreographic protocol layer
├── protocols.rs                    # CvSync, DeltaSync, OpBroadcast choreographies
├── meet_protocols.rs               # ConstraintSync, CapabilityRestriction choreographies
├── composition.rs                  # Protocol composition and execution utilities
└── mod.rs                          # Re-exports

aura-journal/src/semilattice/       # Application semilattice layer
├── journal_map.rs                  # JournalMap as CvState implementation
├── account_state.rs                # Modern AccountState using semilattice composition
├── concrete_types.rs               # Domain-specific CRDT types (DeviceRegistry, etc.)
├── meet_types.rs                   # Domain-specific meet CRDTs (CapabilitySet, etc.)
├── tests/                          # Integration tests
│   └── meet_integration.rs         # End-to-end meet CRDT scenario tests
└── mod.rs                          # Journal-specific re-exports
```

### 10.3 Component Specifications

**Foundation Layer** (`aura-types/src/semilattice/`):
- `JoinSemilattice`, `Bottom`, `CvState` traits for join-based CRDTs
- `MeetSemiLattice`, `Top`, `MvState` traits for meet-based CRDTs
- `CausalOp`, `CmApply`, `Dedup` traits for operation-based CRDTs  
- `StateMsg<S>`, `MeetStateMsg<S>`, `DeltaMsg<D>`, `OpWithCtx<Op,Ctx>` message types
- `ConstraintMsg<C>`, `ConsistencyProof` for meet-based protocols
- Common trait implementations for primitive types and duality mappings

**Effect Layer** (`aura-protocol/src/effects/semilattice/`):
- `CvHandler<S>`: Enforces `state := state.join(&received_state)` law for accumulative semantics
- `MvHandler<S>`: Enforces `state := state.meet(&constraint)` law for restrictive semantics  
- `CmHandler<S,Op>`: Enforces causal ordering and deduplication laws
- `DeltaHandler<S,D>`: Accumulates deltas and folds into state
- `MultiConstraintHandler<S>`: Manages constraints across different scopes
- Delivery effects for causal broadcast and gossip protocols

**Choreographic Layer** (`aura-choreography/src/semilattice/`):
- `CvSync` choreography: `μX . (r → * : StateMsg<S> . X)` for join-based sync
- `ConstraintSync` choreography: constraint propagation and intersection protocols
- `CapabilityRestriction` choreography: capability intersection with verification
- `OpBroadcast` choreography: `μX . r ⊳ { issue : r → * : OpWithCtx . X, idle : end }`
- Execution functions that bridge session types with effect handlers

**Application Layer** (`aura-journal/src/semilattice/`):
- `JournalMap` implementing `CvState` for ops + intent staging
- `ModernAccountState` composing multiple semilattice CRDTs for account management
- `DeviceRegistry`, `IntentPool`, `EpochLog` domain-specific join CRDTs
- `CapabilitySet`, `TimeWindow`, `SecurityPolicy` domain-specific meet CRDTs
- Integration with choreographic synchronization protocols
- Migration utilities from legacy Automerge-based implementations

### 10.4 Usage Pattern

```rust
// Foundation types - Join-based CRDTs
use aura_types::semilattice::{StateMsg, CvState};

// Foundation types - Meet-based CRDTs  
use aura_types::semilattice::{MeetStateMsg, ConstraintMsg, MvState};

// Effect handlers
use aura_protocol::effects::semilattice::{CvHandler, MvHandler};

// Choreographic execution  
use aura_choreography::semilattice::{execute_cv_sync, execute_constraint_sync};

// Application CRDTs
use aura_journal::semilattice::{JournalMap, CapabilitySet, ModernAccountState};

// Complete integration - Join-based synchronization
let mut cv_handler = CvHandler::<JournalMap>::new();
execute_cv_sync(adapter, replicas, my_role, &mut cv_handler).await?;

// Complete integration - Meet-based constraint coordination
let mut mv_handler = MvHandler::<CapabilitySet>::new();
execute_constraint_sync(adapter, constraint, participants, my_device_id).await?;

// Unified account management
let account_state = ModernAccountState::new(account_id, group_key);
```

This architecture achieves **composable semilattices on session types** where:
- Messages are precisely typed as session payloads (`T`)
- Effect handlers enforce semilattice semantic laws independently  
- Choreographies define communication patterns algebraically
- Both accumulative (join) and restrictive (meet) semantics are supported
- Application semilattices integrate seamlessly with the unified journal
- Legacy CRDT implementations can migrate to the harmonized system

---

## 11. Conclusion

Aura expresses semilattice replication as a system of role explicit session types combined with effect handlers. The global types describe communication among roles, and projection gives each role a local binary type. This structure enforces safety and keeps the protocol definition clear.

Convergence arises from the handler logic and the delivery effects. The grammar defines message flow and ordering, while the effect layer enforces semilattice operations (both join and meet) and causal order. Execution connects to the algebra through these effects rather than intents.

This unified approach gives Aura a complete algebraic foundation for distributed state management through both accumulative and restrictive semantics. It keeps network safety and algebraic correctness aligned while supporting reusable, compositional protocol definitions that span the full spectrum from growth-oriented CRDTs to constraint-oriented meet semi-lattices.
