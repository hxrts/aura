# Aura Theoretical Foundations

This document establishes the complete mathematical foundation for Aura's distributed system architecture. It presents the formal calculus, algebraic/session types, and semilattice semantics that underlie all system components.

## Overview

Aura's theoretical foundation rests on four mathematical pillars:

1. **Aura Calculus (𝒜)** - The core computational model unifying communication, state, and trust
2. **Algebraic Types** - Semilattice-structured state with monotonic properties
3. **Multi-Party Session Types** - Choreographic protocols with safety guarantees
4. **CRDT Semantics** - Conflict-free replication with convergence proofs

These form a privacy-preserving, spam-resistant, capability-checked distributed λ-calculus with a unified information flow budget.

---

## 1. Aura Calculus (𝒜)

### 1.1 Syntax

We define programs as *effectful, session-typed processes* operating over semilattice-structured state.

```
Terms:
  e ::= v | e₁ e₂ | handle e with H | send κ m | recv κ | merge δ | refine γ

Facts (Join-Semilattice):
  (F, ⊔, ⊥)        where   x ⊔ y = y ⊔ x , x ⊔ (y ⊔ z) = (x ⊔ y) ⊔ z , x ⊔ x = x

Capabilities (Meet-Semilattice):
  (C, ⊓, ⊤)        where   x ⊓ y = y ⊓ x , x ⊓ (y ⊓ z) = (x ⊓ y) ⊓ z , x ⊓ x = x

Contexts:
  κ ∈ Ctx = { DKD(app, label) | RID(A,B) | GID(G,k) }

Messages:
  m ::= ⟨κ, T, σ⟩    // context, typed payload, signature/auth tag

Message extraction functions (used by operational rules):
  facts(m) : Fact     // join-contribution carried by message payload
  caps(m)  : Cap      // meet-contribution (constraint) carried by message payload
```

A process configuration:
```
⟨ P , F , C , κ ⟩
```
represents a running session with fact-state `F`, capability frontier `C`, and privacy context `κ`.

### 1.2 Judgments

```
Γ ⊢ e : T | ϵ
```
means: under typing context `Γ`, expression `e` has type `T` and may perform effects `ϵ`.

Effect set:
```
ϵ ::= ∅ | ϵ ∪ {Merge ΔF} | ϵ ∪ {Refine ΔC} | ϵ ∪ {Send κ m} | ϵ ∪ {Recv κ}
```

### 1.3 Operational Semantics

**State evolution:**
```
(Merge)
⟨ merge δ , F , C , κ ⟩ → ⟨ unit , F ⊔ δ , C , κ ⟩

(Refine)
⟨ refine γ , F , C , κ ⟩ → ⟨ unit , F , C ⊓ γ , κ ⟩
```

**Capability-guarded actions:**
Each side effect or message action `a` carries a required capability predicate `need(a)`.

```
(Send)
if  need(m) ≤ C ∧ headroom(κ, cost, F, C)    then  ⟨ send κ m , F , C , κ ⟩ → ⟨ unit , F , C , κ ⟩
else block

(Recv)
⟨ recv κ , F , C , κ ⟩ → ⟨ m , F ⊔ facts(m) , C ⊓ caps(m) , κ ⟩
```

**Context isolation:**
No reduction may combine messages of distinct contexts:
```
if κ₁ ≠ κ₂ then  send κ₁ m  ∥  recv κ₂  ≡  blocked
```

Here `headroom(κ, cost, F, C)` is shorthand for the budget predicate derived from journal facts and capability limits for context κ:
```
headroom(κ, cost, F, C) ≜ spent_κ(F) + cost ≤ limit_κ(C)
```
Implementations realize this by merging a `FlowBudget` charge fact before `send` (see §2.3 and §5.3), so the side condition is enforced by the same monotone laws as other effects.

### 1.4 Algebraic Laws (Invariants)

1. **Monotonic Growth:** `Fₜ₊₁ = Fₜ ⊔ δ` → `Fₜ ≤ Fₜ₊₁`
2. **Monotonic Restriction:** `Cₜ₊₁ = Cₜ ⊓ γ` → `Cₜ₊₁ ≤ Cₜ`
3. **Safety:** Every side effect `σ` requires `need(σ) ≤ C`.
4. **Context Separation:** For any two contexts κ₁, κ₂, no observable trace relates their internal state unless a *bridge protocol* is typed for (κ₁, κ₂).
5. **Compositional Confluence:**
   - `(merge δ₁ ; merge δ₂) ≡ merge(δ₁ ⊔ δ₂)`
   - `(refine γ₁ ; refine γ₂) ≡ refine(γ₁ ⊓ γ₂)`

---

## 2. Core Algebraic Types

### 2.1 Foundation Objects

```rust
// Capabilities are meet-semilattice elements (refinement only shrinks them).
type Cap     // partially ordered set (≤), with meet ⊓ and top ⊤
type Policy  // same carrier as Cap, different role (policy-as-capability)

// Facts are join-semilattice elements (accumulation only grows them).
type Fact    // partially ordered set (≤), with join ⊔ and bottom ⊥

// Journal state is a CRDT over Facts; Revocations / Constraints are Caps.
struct Journal {
  facts: Fact,            // Cv/Δ/CmRDT carrier with ⊔
  caps:  Cap,             // capability frontier with ⊓
}

// Relationship-scoped keys (pairwise or group) define privacy contexts.
struct ContextId;   // derived (DKD) identifiers
struct RID;         // pairwise secret context (X25519-derived)
struct GID;         // group secret context (threshold-derived)
struct Epoch(u64);  // monotone, context-scoped
struct FlowBudget { limit: u64, spent: u64, epoch: Epoch };
struct Receipt { ctx: ContextId, src: DeviceId, dst: DeviceId, epoch: Epoch, cost: u32, nonce: u64, prev: Hash32, sig: Signature };

// Typed messages carry effects and proofs under a context.
struct Msg<Ctx, Payload, Version> {
  ctx: Ctx,                 // RID or GID or DKD-context
  payload: Payload,         // typed by protocol role/state
  ver: Version,             // semantic version nego
  auth: AuthTag,            // signatures/MACs/AEAD tags
}
```

**Intuition:**
- **`Fact`** models "what we know" (⊔-monotone)
- **`Cap`** models "what we may do" (⊓-monotone)
- **`Journal`** is the *pullback* where growing facts and shrinking capabilities meet
- **Contexts** (RID/GID/DKD) induce *privacy partitions*; messages never cross partitions without explicit re-derivation/translation

### 2.2 Content Addressing Contract

All Aura artifacts - facts, snapshot blobs, cache metadata, upgrade manifests - are identified by the hash of their canonical encoding:

- **Canonical encoding**: Structures are serialized using canonical CBOR (sorted maps, deterministic integer width). We call the helper `hash_canonical(bytes)` whenever we need a digest.
- **Immutable identifiers**: Once a digest is published, the bytes for that artifact MUST NOT change. New content implies a new digest and a new fact in the journal.
- **Off-chain artifacts**: Snapshots or upgrade bundles stored outside the journal are referenced solely by their digest; downloaders verify the digest before accepting the payload.
- **Verification**: Journal merges compare digests; mismatches are rejected before state is updated.

### 2.3 Effect Signatures

Core effect families provide the runtime contract:

```rust
// Read/append mergeable state
effect JournalEffects {
  read_facts   : () -> Fact
  merge_facts  : Fact -> ()
  read_caps    : () -> Cap
  refine_caps  : Cap  -> ()       // meet: caps := caps ⊓ arg
}

// Cryptography and key mgmt (abstracted to swap FROST, AEAD, DR, etc.)
effect CryptoEffects {
  sign_threshold  : Bytes -> SigWitness
  aead_seal       : (K_box, Plain) -> Cipher
  aead_open       : (K_box, Cipher) -> Plain?
  ratchet_step    : RID/GID -> RID/GID
}

// Transport (unified)
effect TransportEffects {
  send    : (PeerId, Msg<Ctx, P, V>) -> ()
  recv    : () -> Msg<Ctx, Any, V>
  connect : PeerId -> Channel
}
```

### 2.4 Guards and Observability Invariants

Every observable side effect is mediated by a guard chain:

1. CapGuard: `need(σ) ≤ Caps(ctx)`
2. FlowGuard: `headroom(ctx, cost)` where `charge(ctx, peer, cost, epoch)` succeeds and yields a `Receipt`
3. JournalCoupler: commit of attested facts is atomic with the send

Named invariants used across documents:
- Charge‑Before‑Send: FlowGuard must succeed before any transport send.
- No‑Observable‑Without‑Charge: there is no `send(ctx, peer, …)` event without a preceding successful `charge(ctx, peer, cost, epoch)`.
- Deterministic‑Replenishment: `limit(ctx)` updates via meet on deterministic journal facts; `spent` is join‑monotone; epochs gate resets.

```rust
// Time & randomness for simulation/proofs
effect TimeEffects   { now : () -> Instant; sleep : Duration -> () }
effect RandEffects   { sample : Dist -> Val }

// Privacy budgets (ext/ngh/group observers)
effect LeakageEffects {
  record_leakage   : (ObserverClass, f64) -> ()
  remaining_budget : ObserverClass -> f64
}
```

`LeakageEffects` is the runtime hook that enforces the `[leak: (ℓ_ext, ℓ_ngh, ℓ_grp)]` annotations introduced in the session grammar. Its concrete implementation lives in `crates/aura-protocol/src/guards/privacy.rs` and is wired through the effect system so choreographies cannot exceed configured budgets.

### Information Flow Budgets (Spam + Privacy)

To couple spam resistance with privacy guarantees, each context pair `(Ctx, Peer)` carries a **flow budget**:

```rust
struct FlowBudget {
    spent: u64,   // monotone counter (join = max)
    limit: u64,   // capability-style guard (meet = min)
}
```

- Budgets live in the journal beside capability facts and therefore inherit the same semilattice laws (`spent` only grows, `limit` only shrinks).
- Sending a message deducts a fixed `flow_cost` from the local budget before the effect executes; if `spent + flow_cost > limit`, the effect runtime blocks the send.
- Replenishment happens through explicit `BudgetUpdate` facts emitted during epoch-rotation choreographies. Because updates are facts, every replica converges on the same `limit` value without side channels.
- Multi-hop forwarding charges budgets hop-by-hop. Relays attach a signed `Receipt` that proves the previous hop still had headroom; receipts are scoped to the same context so they never leak to unrelated observers.

This structure lets us express “who may talk, how often, and with what metadata leakage” using the same monotone calculus that already governs capabilities and leakage.

### 2.4 Semantic Laws

**Join laws (facts):**
- Associative, commutative, idempotent
- **Monotonicity:** if `F₀ = read_facts()` and after `merge_facts(f)` we have `F₁`, then `F₀ ≤ F₁` (with respect to the facts partial order)

**Meet laws (caps):**
- Associative, commutative, idempotent
- **Safety monotonicity:** `refine_caps c` never increases authority

**Non-interference (cap-guarded effects):**
- For any effect `e` guarded by capability predicate `Γ ⊢ e : allowed`, executing `e` from `caps = C` is only permitted if `C ⊓ need(e) = need(e)`

**Context isolation:**
- If two contexts `Ctx1 ≠ Ctx2` are not explicitly bridged by a typed protocol, **no** `Msg<Ctx1, …>` flows into `Ctx2`

---

## 3. Multi-Party Session Type Algebra

### 3.1 Global Type Grammar (G)

The global choreography type describes the entire protocol from a bird's-eye view. Aura extends vanilla MPST with capability guards, journal coupling, and leakage budgets:

```
G ::= r₁ → r₂ : T [guard: Γ, ▷ Δ, leak: L] . G   // Point-to-point send
    | r → * : T [guard: Γ, ▷ Δ, leak: L] . G     // Broadcast (one-to-many)
    | G ∥ G                                      // Parallel composition
    | r ⊳ { ℓᵢ : Gᵢ }ᵢ∈I                         // Choice (role r decides)
    | μX . G                                     // Recursion
    | X                                          // Recursion variable
    | end                                        // Termination

T ::= Unit | Bool | Int | String | ...           // Message types
r ::= Role identifiers (Alice, Bob, ...)
ℓ ::= Label identifiers (accept, reject, ...)
Γ ::= meet-closed predicate `need(m) ≤ caps_r(ctx)`
Δ ::= journal delta (facts) merged around the message
L ::= leakage tuple `(ℓ_ext, ℓ_ngh, ℓ_grp)`
```

**Conventions:**
- `r₁ → r₂ : T [guard: Γ, ▷ Δ, leak: L] . G` means "role r₁ checks `Γ`, applies Δ, records leakage L, sends T to r₂, then continues with G."
- `r → * : …` performs the same sequence for broadcasts.
- `G₁ ∥ G₂` means "execute G₁ and G₂ concurrently."
- `r ⊳ { ℓᵢ : Gᵢ }` means "role r decides which branch ℓᵢ to take, affecting all participants."
- `μX . G` binds recursion variable X in G.

Note on Δ: the journal delta may include budget‑charge updates (incrementing `spent` for the active epoch) and receipt acknowledgments. Projection ensures these updates occur before any transport effect so “no observable without charge” holds operationally.

### 3.2 Local Type Grammar (L)

After projection, each role executes a local session type (binary protocol) augmented with effect sequencing:

```
L ::= do E . L                           // Perform effect (merge, guard, leak)
    | ! T . L                            // Send (output)
    | ? T . L                            // Receive (input)
    | ⊕ { ℓᵢ : Lᵢ }ᵢ∈I                   // Internal choice (select)
    | & { ℓᵢ : Lᵢ }ᵢ∈I                   // External choice (branch)
    | μX . L                             // Recursion
    | X                                  // Recursion variable
    | end                                // Termination

E ::= merge(Δ) | check_caps(Γ) | refine_caps(Γ) | record_leak(L) | noop
```

### 3.3 Projection Function (π)

The projection function `πᵣ(G)` extracts role r's local view from global choreography G:

By convention, an annotation `▷ Δ` at a global step induces per-side deltas `Δ_send` and `Δ_recv`. Unless otherwise specified by a protocol, we take `Δ_send = Δ_recv = Δ` (symmetric journal updates applied at both endpoints).

```
πᵣ(r₁ → r₂ : T [guard: Γ, ▷ Δ, leak: L] . G) =
    do merge(Δ_send) ; do check_caps(Γ) ; do record_leak(L) ; ! T . πᵣ(G)   if r = r₁
    do merge(Δ_recv) ; do refine_caps(Γ) ; do record_leak(L) ; ? T . πᵣ(G)  if r = r₂
    πᵣ(G)                                                                     otherwise

πᵣ(s → * : T [guard: Γ, ▷ Δ, leak: L] . G) =
    do merge(Δ_send) ; do check_caps(Γ) ; do record_leak(L) ; ! T . πᵣ(G)   if r = s
    do merge(Δ_recv) ; do refine_caps(Γ) ; do record_leak(L) ; ? T . πᵣ(G)  otherwise

πᵣ(G₁ ∥ G₂) =
    πᵣ(G₁) ⊙ πᵣ(G₂)      where ⊙ is merge operator
                          (sequential interleaving if no conflicts)

πᵣ(r' ⊳ { ℓᵢ : Gᵢ }) =
    ⊕ { ℓᵢ : πᵣ(Gᵢ) }     if r = r' (decider)
    & { ℓᵢ : πᵣ(Gᵢ) }     if r ≠ r' (observer)

πᵣ(μX . G) =
    μX . πᵣ(G)            if πᵣ(G) ≠ end
    end                   if πᵣ(G) = end

πᵣ(X) = X
πᵣ(end) = end
```

### 3.4 Duality and Safety

For binary session types, duality ensures complementary behavior:

```
dual(! T . L) = ? T . dual(L)
dual(? T . L) = ! T . dual(L)
dual(⊕ { ℓᵢ : Lᵢ }) = & { ℓᵢ : dual(Lᵢ) }
dual(& { ℓᵢ : Lᵢ }) = ⊕ { ℓᵢ : dual(Lᵢ) }
dual(μX . L) = μX . dual(L)
dual(X) = X
dual(end) = end
```

**Property**: If Alice's local type is L, then Bob's local type is dual(L) for their communication to be type-safe.

### 3.5 Session Type Safety Guarantees

The projection process ensures:

1. **Deadlock Freedom**: No circular dependencies in communication
2. **Type Safety**: Messages have correct types at send/receive
3. **Communication Safety**: Every send matches a receive
4. **Progress**: Protocols always advance (no livelocks)
5. **Agreement**: All participants agree on the chosen branch and protocol state (modulo permitted interleavings of independent actions)

### 3.6 Turing Completeness vs Safety Restrictions

The MPST algebra is Turing complete when recursion (`Rec`/`Var`) is unrestricted. However, well-typed programs intentionally restrict expressivity to ensure critical safety properties:

- **Termination**: Protocols that always complete (no infinite loops)
- **Deadlock Freedom**: No circular waiting on communication
- **Progress**: Protocols always advance to next state

Rumpsteak balances expressivity and safety through guarded recursion constructs.

### 3.7 Runtime Bridge (Where It Lives)

The projection and interpretation machinery described above is scaffolded in the following modules:

- `crates/aura-core/src/sessions.rs` defines the global choreography AST used by proc-macros.
- `crates/aura-mpst/src/runtime.rs` contains the projection/interpreter glue for `rumpsteak_aura::try_session`.
- `crates/aura-protocol/src/handlers/rumpsteak_handler.rs` and `crates/aura-protocol/src/choreography/runtime/aura_handler_adapter.rs` bridge projected locals into the unified effect system.

When adding a new protocol, place the choreography in `crates/aura-protocol/src/choreography/protocols/` and let the handler pipeline route effect calls into `AuraEffectSystem`.

---

### 3.8 Free Algebra View (Choreography as Initial Object)

You can think of the choreography language as a small set of protocol-building moves:

Generators:
  - `Send(r₁, r₂, T, [guard: Γ, ▷ Δ, leak: L])`
  - `Broadcast(r, R*, T, [guard: Γ, ▷ Δ, leak: L])`
  - `Parallel(G₁, …, Gₙ)`
  - `Choice(r, {ℓᵢ ↦ Gᵢ}ᵢ∈I)`
  - `Rec(X, G)` and `Var(X)`
  - `End`

Taken together, these moves form a “free algebra”: the language carries just enough structure to compose protocols, but no extra operational behavior. The effect runtime is the target algebra that gives these moves concrete meaning.

Projection (from a global protocol to each role) followed by interpretation (running it against the effect runtime) yields one canonical way to execute any choreography.

The “free” (initial) property is what keeps this modular. Because the choreographic layer only expresses structure, any effect runtime that respects those composition laws admits exactly one interpretation of a given protocol. This allows swapping or layering handlers without changing choreographies.

The system treats computation and communication symmetrically. A step is the same transform whether it happens locally or across the network. If the sender and receiver are the same role, the projection collapses the step into a local effect call. If they differ, it becomes a message exchange with the same surrounding journal/guard/leak actions. Protocol authors write global transforms, the interpreter decides local versus remote at time of projection.

---

### 3.9 Algebraic Effects and the Interpreter

Aura treats protocol execution as interpretation over an algebraic effect interface. After projecting a global choreography to each role, a polymorphic interpreter walks the role’s AST and dispatches each operation to `AuraEffectSystem` via handlers and middleware. The core actions are exactly the ones defined by the calculus and effect signatures in this document: `merge` (facts grow by ⊔), `refine` (caps shrink by ⊓), `send`/`recv` (context-scoped communication), and leakage/budget metering. The interpreter enforces the lattice laws and guard predicates while executing these actions in the order dictated by the session type.

Because the interface is algebraic, there is a single semantics regardless of execution strategy. This enables two interchangeable modes:

- Static compilation: choreographies lower to direct effect calls with zero runtime overhead.
- Dynamic interpretation: choreographies execute through the runtime interpreter for flexibility and tooling.

Both preserve the same program structure and checks; the choice becomes an implementation detail. This also captures the computation/communication symmetry: a choreographic step describes a typed transform. If the sender and receiver are the same role, projection collapses the step to a local effect invocation. If they differ, the interpreter performs a network send/receive with the same surrounding `merge`/`check_caps`/`refine`/`record_leak` sequence. Protocol authors reason about transforms, the interpreter decides locality at projection time.

---

## 4. CRDT Semantic Foundations

### 4.1 CRDT Semantic Interfaces

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

// Meet-based CRDTs (constraints)
pub trait MeetSemilattice: Clone { fn meet(&self, other: &Self) -> Self; }
pub trait Top { fn top() -> Self; }
pub trait MvState: MeetSemilattice + Top {}
```

### 4.2 Message Types for CRDTs

```rust
// Phantom tags for clarity (optional at runtime)
#[derive(Clone)] pub enum MsgKind { FullState, Delta, Op, Constraint }

pub type StateMsg<S> = (S, MsgKind);      // (payload, tag::FullState)
pub type DeltaMsg<D> = (D, MsgKind);      // (payload, tag::Delta)
pub type ConstraintMsg<C> = (C, MsgKind); // (payload, tag::Constraint)

#[derive(Clone)] pub struct OpWithCtx<Op, Ctx> { pub op: Op, pub ctx: Ctx }

// Auxiliary protocol payloads
pub type Digest<Id> = Vec<Id>;
pub type Missing<Op> = Vec<Op>;
```

### 4.3 CRDT Protocol Schemas

**CvRDT (State-based Anti-Entropy):**
CvRDTs synchronize by state exchange. Each replica periodically sends its full state to others, who merge it using the join operation.

```
CvSync<S> := μX . (A → B : State<S> . X) ∥ (B → A : State<S> . X)
```

**Δ-CRDT (Delta-based Gossip):**
Δ-CRDTs optimize CvRDTs by transmitting deltas rather than full states.

```
DeltaSync<Δ> := μX . (A → B : DeltaMsg<Δ> . X) ∥ (B → A : DeltaMsg<Δ> . X)
```

**CmRDT (Operation-based):**
CmRDTs propagate operations with causal broadcast guarantees.

```
OpBroadcast<Op, Ctx> := μX . ( r ⊳ {
  issue : r → * : OpWithCtx<Op, Ctx> . X,
  idle  : end
} )
```

**Meet-based Constraint Propagation:**
Meet CRDTs handle constraint intersection and capability refinement.

```
ConstraintSync<C> := μX . (A → B : ConstraintMsg<C> . X) ∥ (B → A : ConstraintMsg<C> . X)
```

### 4.4 Convergence Properties

**Safety & Convergence:**
- **Session safety**: Projection ensures dual locals, communication safety, and deadlock freedom
- **Cv/Δ convergence**: eventual delivery + semilattice laws ⇒ states converge to the join of all local updates
- **Cm convergence**: causal delivery + dedup + commutative ops ⇒ replicas converge modulo permutation of independent ops
- **Meet convergence**: constraint propagation + meet laws ⇒ capabilities converge to intersection of all constraints

---

## 5. Information Flow Contract (Privacy + Spam)

### 5.1 Privacy Layers

For any trace `τ` of observable messages:

1. **Unlinkability:** ∀ κ₁ ≠ κ₂, `τ[κ₁↔κ₂] ≈_ext τ`
2. **Non-amplification:** Information visible to observer class `o` is monotone in authorized capabilities:
   ```
   I_o(τ₁) ≤ I_o(τ₂)  iff  C_o(τ₁) ≤ C_o(τ₂)
   ```
3. **Leakage Bound:** For each observer `o`, `L(τ,o) ≤ Budget(o)`.
4. **Flow Budget Soundness (Named):**
   - Charge‑Before‑Send
   - No‑Observable‑Without‑Charge
   - Deterministic‑Replenishment
   - Convergence: Within a fixed epoch and after convergence, `spent_κ ≤ min_r limit_κ^r` across replicas `r`.

### 5.2 Web-of-Trust Model

Let `W = (V, E)` where vertices are accounts; edges carry relationship contexts and delegation fragments.

- Each edge `(A,B)` defines a **pairwise context** `RID_AB` with derived keys
- Delegations are meet-closed elements `d ∈ Cap`, scoped to contexts
- The **effective capability** at A is:
  ```
  Caps_A = (LocalGrants_A ⊓ ⋂_{(A,x)∈E} Delegation_{x→A}) ⊓ Policy_A
  ```

**WoT invariants:**
- **Compositionality:** Combining multiple delegations uses ⊓ (never widens)
- **Local sovereignty:** `Policy_A` is always in the meet; A can only reduce authority further
- **Projection:** For any protocol projection to A, guard checks refer to `Caps_A(ctx)`

### 5.3 Flow Budget Contract

The unified information‑flow budget regulates emission rate/volume and observable leakage using the same semilattice laws as capabilities and facts. For any context `κ` and peer `p`:

1. Charge‑Before‑Send: A send or forward is permitted only if a budget charge succeeds first. If charging fails, the step blocks locally and emits no network observable.
2. No‑Observable‑Without‑Charge: For any trace `τ`, there is no event labeled `send(κ, p, …)` without a preceding successful charge for `(κ, p)` in the same epoch.
3. Receipt soundness: A relay accepts a packet only with a valid per‑hop `Receipt` (context‑scoped, epoch‑bound, signed) and sufficient local headroom; otherwise it drops locally.
4. Deterministic replenishment: `limit_κ` updates are deterministic functions of journal facts and converge via meet; `spent_κ` is join‑monotone. Upon epoch rotation, `spent_κ` resets and receipts rebind to the new epoch.
5. Context scope: Budget facts and receipts are scoped to `κ`; they neither leak nor apply across distinct contexts (non‑interference).
6. Composition with caps: A transport effect requires both `need(m) ≤ C` and `headroom(κ, cost, F, C)` (see §1.3). Either guard failing blocks the effect.
7. Convergence bound: Within a fixed epoch and after convergence, `spent_κ ≤ min_r limit_κ^r` across replicas `r`.

---

## 6. Application Model

Every distributed protocol `G` is defined as a multi-party session type with role projections:

```
G ::= μ X.
       A → B : m<T> [guard need(m) ≤ C_A, update F_A ⊔= ΔF, refine C_B ⊓= ΔC]
       ; X
```

When executed, each role `ρ` instantiates a handler:

```
handle protocol(G, ρ) with { on_send, on_recv, on_merge, on_refine }
```

Handlers compose algebraically over `(F,C)` by distributing operations over semilattice state transitions. This yields an *effect runtime* capable of:

- key-ceremony coordination (threshold signatures)
- gossip and rendezvous (context-isolated send/recv)
- distributed indexing (merge facts, meet constraints)
- garbage collection (join-preserving retractions)

---

## 7. Interpretation

Under this calculus, we can make the following interpretation:

### The Semilattice Layer

The **join-semilattice (Facts)** captures evidence and observations (trust and information flow). Examples: delegations/attestations, quorum proofs, ceremony transcripts, flow receipts, and monotone `spent` counters.

The **meet-semilattice (Capabilities)** captures enforcement limits and constraints (trust and information flow). Examples: local policy, revocations, capability constraints, per-context `limit` budgets, leak bounds, and consent gates.

Effective authority and headroom are computed from both lattices: `C_eff(F,C) = derive_caps(F) ⊓ C ⊓ Policy`; `headroom(F,C)` uses `limit ∈ C` and `spent ∈ F`, permitting sends iff `spent + cost ≤ limit`.

### The Session-Typed Process Layer

This layer guarantees *communication safety* and *progress*. It projects global types with annotations `[guard: Γ, ▷ Δ, leak: L]` into local programs, ensuring deadlock freedom, communication safety, branch agreement, and aligning capability checks, journal updates, and leakage accounting with each send/recv.

### The Effect Handler Layer

The Effect Handler system provides *operational semantics and composability*. It realizes `merge/refine/send/recv` as algebraic effects, enforces lattice monotonicity (⊔ facts, ⊓ caps), guard predicates, and budget/leakage metering, and composes via middleware across crypto, storage, and transport.

### The Privacy Contract

The privacy contract defines *which transitions are observationally equivalent*. Under context isolation and budgeted leakage, traces that differ only by in-context reorderings or by merges/refinements preserving observer-class budgets and effective capabilities are indistinguishable. No cross-context flow occurs without a typed bridge.

Together, these form a *privacy-preserving, capability-checked distributed λ-calculus*.

## See Also

- `000_overview.md` - Overall project architecture and goals
- `002_system_architecture.md` - Implementation patterns and system design
- `003_distributed_applications.md` - Concrete applications and examples
- `103_information_flow_budget.md` - Unified budget model for privacy + spam
