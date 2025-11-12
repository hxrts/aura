# Aura Theory

This document establishes the complete mathematical foundation for Aura's distributed system architecture. It presents the formal calculus, algebraic/session types, and semilattice semantics that underlie all system components.

## Overview

Aura's theoretical foundation rests on four mathematical pillars:

1. Aura Calculus ($\mathcal{A}$) provides the core computational model unifying communication, state, and trust.
2. Algebraic Types structure state as semilattices with monotonic properties.
3. Multi-Party Session Types specify choreographic protocols with safety guarantees.
4. CRDT Semantics enable conflict-free replication with convergence proofs.

The combination forms a privacy-preserving, spam-resistant, capability-checked distributed λ-calculus. The system enforces a unified information flow budget across all operations.

## 1. Aura Calculus ($\mathcal{A}$)

### 1.1 Syntax

We define programs as *effectful, session-typed processes* operating over semilattice-structured state.

**Terms:**
$$e ::= v \mid e_1\ e_2 \mid \text{handle}\ e\ \text{with}\ H \mid \text{send}\ \kappa\ m \mid \text{recv}\ \kappa \mid \text{merge}\ \delta \mid \text{refine}\ \gamma$$

**Facts (Join-Semilattice):**
$$(F, \sqcup, \bot) \quad \text{where} \quad x \sqcup y = y \sqcup x,\ x \sqcup (y \sqcup z) = (x \sqcup y) \sqcup z,\ x \sqcup x = x$$

**Capabilities (Meet-Semilattice):**
$$(C, \sqcap, \top) \quad \text{where} \quad x \sqcap y = y \sqcap x,\ x \sqcap (y \sqcap z) = (x \sqcap y) \sqcap z,\ x \sqcap x = x$$

**Contexts:**
$$\kappa \in \text{Ctx} = \{ \text{DKD}(\text{app}, \text{label}) \mid \text{RID}(A,B) \mid \text{GID}(G,k) \}$$

**Messages:**
$$m ::= \langle \kappa, T, \sigma \rangle \quad \text{// context, typed payload, signature/auth tag}$$

**Message extraction functions (used by operational rules):**
$$\text{facts}(m) : \text{Fact} \quad \text{// join-contribution carried by message payload}$$
$$\text{caps}(m) : \text{Cap} \quad \text{// meet-contribution (constraint) carried by message payload}$$

A process configuration:
$$\langle P, F, C, \kappa \rangle$$
represents a running session with fact-state $F$, capability frontier $C$, and privacy context $\kappa$.

### 1.2 Judgments

$$\Gamma \vdash e : T \mid \epsilon$$
means: under typing context $\Gamma$, expression $e$ has type $T$ and may perform effects $\epsilon$.

**Effect set:**
$$\epsilon ::= \emptyset \mid \epsilon \cup \{\text{Merge}\ \Delta F\} \mid \epsilon \cup \{\text{Refine}\ \Delta C\} \mid \epsilon \cup \{\text{Send}\ \kappa\ m\} \mid \epsilon \cup \{\text{Recv}\ \kappa\}$$

### 1.3 Operational Semantics

**State evolution:**

$$(Merge)\quad \langle \text{merge}\ \delta, F, C, \kappa \rangle \to \langle \text{unit}, F \sqcup \delta, C, \kappa \rangle$$

$$(Refine)\quad \langle \text{refine}\ \gamma, F, C, \kappa \rangle \to \langle \text{unit}, F, C \sqcap \gamma, \kappa \rangle$$

**Capability-guarded actions:**
Each side effect or message action $a$ carries a required capability predicate $\text{need}(a)$.

$$(Send)\quad \text{if}\ \text{need}(m) \leq C \land \text{headroom}(\kappa, \text{cost}, F, C)\ \text{then}\ \langle \text{send}\ \kappa\ m, F, C, \kappa \rangle \to \langle \text{unit}, F, C, \kappa \rangle$$
$$\text{else block}$$

$$(Recv)\quad \langle \text{recv}\ \kappa, F, C, \kappa \rangle \to \langle m, F \sqcup \text{facts}(m), C \sqcap \text{caps}(m), \kappa \rangle$$

**Context isolation:**
No reduction may combine messages of distinct contexts:
$$\text{if}\ \kappa_1 \neq \kappa_2\ \text{then}\ \text{send}\ \kappa_1\ m \parallel \text{recv}\ \kappa_2 \equiv \text{blocked}$$

Here $\text{headroom}(\kappa, \text{cost}, F, C)$ is shorthand for the budget predicate derived from journal facts and capability limits for context $\kappa$:
$$\text{headroom}(\kappa, \text{cost}, F, C) \triangleq \text{spent}_\kappa(F) + \text{cost} \leq \text{limit}_\kappa(C)$$

Implementations realize this by merging a $\text{FlowBudget}$ charge fact before $\text{send}$ (see §2.3 and §5.3), so the side condition is enforced by the same monotone laws as other effects.

### 1.4 Algebraic Laws (Invariants)

1. Monotonic Growth: $F_{t+1} = F_t \sqcup \delta \implies F_t \leq F_{t+1}$
2. Monotonic Restriction: $C_{t+1} = C_t \sqcap \gamma \implies C_{t+1} \leq C_t$
3. Safety: Every side effect $\sigma$ requires $\text{need}(\sigma) \leq C$.
4. Context Separation: For any two contexts $\kappa_1, \kappa_2$, no observable trace relates their internal state unless a bridge protocol is typed for $(\kappa_1, \kappa_2)$.
5. Compositional Confluence:
   - $(\text{merge}\ \delta_1; \text{merge}\ \delta_2) \equiv \text{merge}(\delta_1 \sqcup \delta_2)$
   - $(\text{refine}\ \gamma_1; \text{refine}\ \gamma_2) \equiv \text{refine}(\gamma_1 \sqcap \gamma_2)$

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

The type `Cap` represents capabilities as meet-semilattice elements. Refinement operations can only reduce authority through the meet operation.

The type `Fact` represents facts as join-semilattice elements. Accumulation operations can only add information through the join operation.

The `Journal` struct combines facts and capabilities into a unified CRDT. Facts grow monotonically while capabilities shrink monotonically.

Contexts (`RID`, `GID`, `ContextId`) define privacy partitions. Messages never cross partition boundaries without explicit protocol support.

### 2.2 Content Addressing Contract

All Aura artifacts are identified by the hash of their canonical encoding. This includes facts, snapshot blobs, cache metadata, and upgrade manifests.

Structures are serialized using canonical CBOR with sorted maps and deterministic integer width. The helper function $\text{hash\_canonical}(\text{bytes})$ computes digests when needed.

Once a digest is published, the bytes for that artifact cannot change. New content requires a new digest and a new fact in the journal.

Snapshots and upgrade bundles stored outside the journal are referenced solely by their digest. Downloaders verify the digest before accepting the payload. Journal merges compare digests and reject mismatches before updating state.

### 2.3 Effect Signatures

Core effect families provide the runtime contract:

```purescript
-- Read/append mergeable state
class JournalEffects m where
  read_facts   :: m Fact
  merge_facts  :: Fact -> m Unit
  read_caps    :: m Cap
  refine_caps  :: Cap -> m Unit       -- meet: caps := caps ⊓ arg

-- Cryptography and key mgmt (abstracted to swap FROST, AEAD, DR, etc.)
class CryptoEffects m where
  sign_threshold  :: Bytes -> m SigWitness
  aead_seal       :: K_box -> Plain -> m Cipher
  aead_open       :: K_box -> Cipher -> m (Maybe Plain)
  ratchet_step    :: RID_or_GID -> m RID_or_GID

-- Transport (unified)
class TransportEffects m where
  send    :: PeerId -> Msg Ctx P V -> m Unit
  recv    :: m (Msg Ctx Any V)
  connect :: PeerId -> m Channel
```

These effect signatures define the interface between protocols and the runtime. The `JournalEffects` family handles state operations. The `CryptoEffects` family handles cryptographic operations. The `TransportEffects` family handles network communication.

### 2.4 Guards and Observability Invariants

Every observable side effect is mediated by a guard chain:

1. CapGuard: $\text{need}(\sigma) \leq \text{Caps}(\text{ctx})$
2. FlowGuard: $\text{headroom}(\text{ctx}, \text{cost})$ where $\text{charge}(\text{ctx}, \text{peer}, \text{cost}, \text{epoch})$ succeeds and yields a $\text{Receipt}$
3. JournalCoupler: commit of attested facts is atomic with the send

Named invariants used across documents:
- Charge-Before-Send: FlowGuard must succeed before any transport send.
- No-Observable-Without-Charge: there is no $\text{send}(\text{ctx}, \text{peer}, \ldots)$ event without a preceding successful $\text{charge}(\text{ctx}, \text{peer}, \text{cost}, \text{epoch})$.
- Deterministic-Replenishment: $\text{limit}(\text{ctx})$ updates via meet on deterministic journal facts. The value $\text{spent}$ is join-monotone. Epochs gate resets.

```purescript
-- Time & randomness for simulation/proofs
class TimeEffects m where
  now   :: m Instant
  sleep :: Duration -> m Unit

class RandEffects m where
  sample :: Dist -> m Val

-- Privacy budgets (ext/ngh/group observers)
class LeakageEffects m where
  record_leakage   :: ObserverClass -> Number -> m Unit
  remaining_budget :: ObserverClass -> m Number
```

The `TimeEffects` and `RandEffects` families support simulation and testing. The `LeakageEffects` family enforces privacy budget constraints.

The `LeakageEffects` implementation is the runtime hook that enforces the $[\text{leak}: (\ell_{\text{ext}}, \ell_{\text{ngh}}, \ell_{\text{grp}})]$ annotations introduced in the session grammar. Its concrete implementation lives in `crates/aura-protocol/src/guards/privacy.rs`. The system wires it through the effect system so choreographies cannot exceed configured budgets.

### Information Flow Budgets (Spam + Privacy)

Each context pair $(\text{Ctx}, \text{Peer})$ carries a flow budget to couple spam resistance with privacy guarantees.

```rust
struct FlowBudget {
    spent: u64,   // monotone counter (join = max)
    limit: u64,   // capability-style guard (meet = min)
}
```

The `FlowBudget` struct tracks message emission through two monotone counters. The `spent` field increases through join operations. The `limit` field decreases through meet operations.

Budgets live in the journal beside capability facts. They inherit the same semilattice laws where `spent` only grows and `limit` only shrinks.

Sending a message deducts a fixed `flow_cost` from the local budget before the effect executes. If $\text{spent} + \text{flow\_cost} > \text{limit}$, the effect runtime blocks the send.

Replenishment happens through explicit `BudgetUpdate` facts emitted during epoch-rotation choreographies. Because updates are facts, every replica converges on the same `limit` value without side channels.

Multi-hop forwarding charges budgets hop-by-hop. Relays attach a signed `Receipt` that proves the previous hop still had headroom. Receipts are scoped to the same context so they never leak to unrelated observers.

### 2.4 Semantic Laws

Join laws apply to facts. These operations are associative, commutative, and idempotent. If $F_0 = \text{read\_facts}()$ and after $\text{merge\_facts}(f)$ we have $F_1$, then $F_0 \leq F_1$ with respect to the facts partial order.

Meet laws apply to capabilities. These operations are associative, commutative, and idempotent. The operation $\text{refine\_caps}\ c$ never increases authority.

Cap-guarded effects enforce non-interference. For any effect $e$ guarded by capability predicate $\Gamma \vdash e : \text{allowed}$, executing $e$ from $\text{caps} = C$ is only permitted if $C \sqcap \text{need}(e) = \text{need}(e)$.

Context isolation prevents cross-context flow. If two contexts $\text{Ctx}_1 \neq \text{Ctx}_2$ are not explicitly bridged by a typed protocol, no $\text{Msg}\langle\text{Ctx}_1, \ldots\rangle$ flows into $\text{Ctx}_2$.

## 3. Multi-Party Session Type Algebra

### 3.1 Global Type Grammar (G)

The global choreography type describes the entire protocol from a bird's-eye view. Aura extends vanilla MPST with capability guards, journal coupling, and leakage budgets:

$$G ::= r_1 \to r_2 : T\ [\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L]\ .\ G \quad \text{// Point-to-point send}$$
$$\mid r \to * : T\ [\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L]\ .\ G \quad \text{// Broadcast (one-to-many)}$$
$$\mid G \parallel G \quad \text{// Parallel composition}$$
$$\mid r \triangleright \{ \ell_i : G_i \}_{i \in I} \quad \text{// Choice (role r decides)}$$
$$\mid \mu X.\ G \quad \text{// Recursion}$$
$$\mid X \quad \text{// Recursion variable}$$
$$\mid \text{end} \quad \text{// Termination}$$

$$T ::= \text{Unit} \mid \text{Bool} \mid \text{Int} \mid \text{String} \mid \ldots \quad \text{// Message types}$$
$$r ::= \text{Role identifiers (Alice, Bob, \ldots)}$$
$$\ell ::= \text{Label identifiers (accept, reject, \ldots)}$$
$$\Gamma ::= \text{meet-closed predicate}\ \text{need}(m) \leq \text{caps}_r(\text{ctx})$$
$$\Delta ::= \text{journal delta (facts) merged around the message}$$
$$L ::= \text{leakage tuple}\ (\ell_{\text{ext}}, \ell_{\text{ngh}}, \ell_{\text{grp}})$$

**Conventions:**
- $r_1 \to r_2 : T\ [\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L]\ .\ G$ means "role $r_1$ checks $\Gamma$, applies $\Delta$, records leakage $L$, sends $T$ to $r_2$, then continues with $G$."
- $r \to * : \ldots$ performs the same sequence for broadcasts.
- $G_1 \parallel G_2$ means "execute $G_1$ and $G_2$ concurrently."
- $r \triangleright \{ \ell_i : G_i \}$ means "role $r$ decides which branch $\ell_i$ to take, affecting all participants."
- $\mu X.\ G$ binds recursion variable $X$ in $G$.

Note on $\Delta$: the journal delta may include budget-charge updates (incrementing $\text{spent}$ for the active epoch) and receipt acknowledgments. Projection ensures these updates occur before any transport effect so "no observable without charge" holds operationally.

### 3.2 Local Type Grammar (L)

After projection, each role executes a local session type (binary protocol) augmented with effect sequencing:

$$L ::= \text{do}\ E\ .\ L \quad \text{// Perform effect (merge, guard, leak)}$$
$$\mid !T\ .\ L \quad \text{// Send (output)}$$
$$\mid ?T\ .\ L \quad \text{// Receive (input)}$$
$$\mid \oplus \{ \ell_i : L_i \}_{i \in I} \quad \text{// Internal choice (select)}$$
$$\mid \& \{ \ell_i : L_i \}_{i \in I} \quad \text{// External choice (branch)}$$
$$\mid \mu X.\ L \quad \text{// Recursion}$$
$$\mid X \quad \text{// Recursion variable}$$
$$\mid \text{end} \quad \text{// Termination}$$

$$E ::= \text{merge}(\Delta) \mid \text{check\_caps}(\Gamma) \mid \text{refine\_caps}(\Gamma) \mid \text{record\_leak}(L) \mid \text{noop}$$

### 3.3 Projection Function ($\pi$)

The projection function $\pi_r(G)$ extracts role $r$'s local view from global choreography $G$:

By convention, an annotation $\triangleright \Delta$ at a global step induces per-side deltas $\Delta_{\text{send}}$ and $\Delta_{\text{recv}}$. Unless otherwise specified by a protocol, we take $\Delta_{\text{send}} = \Delta_{\text{recv}} = \Delta$ (symmetric journal updates applied at both endpoints).

$$\pi_r(r_1 \to r_2 : T\ [\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L]\ .\ G) =$$
$$\begin{cases}
\text{do merge}(\Delta_{\text{send}});\ \text{do check\_caps}(\Gamma);\ \text{do record\_leak}(L);\ !T\ .\ \pi_r(G) & \text{if}\ r = r_1 \\
\text{do merge}(\Delta_{\text{recv}});\ \text{do refine\_caps}(\Gamma);\ \text{do record\_leak}(L);\ ?T\ .\ \pi_r(G) & \text{if}\ r = r_2 \\
\pi_r(G) & \text{otherwise}
\end{cases}$$

$$\pi_r(s \to * : T\ [\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L]\ .\ G) =$$
$$\begin{cases}
\text{do merge}(\Delta_{\text{send}});\ \text{do check\_caps}(\Gamma);\ \text{do record\_leak}(L);\ !T\ .\ \pi_r(G) & \text{if}\ r = s \\
\text{do merge}(\Delta_{\text{recv}});\ \text{do refine\_caps}(\Gamma);\ \text{do record\_leak}(L);\ ?T\ .\ \pi_r(G) & \text{otherwise}
\end{cases}$$

$$\pi_r(G_1 \parallel G_2) = \pi_r(G_1) \odot \pi_r(G_2) \quad \text{where}\ \odot\ \text{is merge operator}$$
$$\text{(sequential interleaving if no conflicts)}$$

$$\pi_r(r' \triangleright \{ \ell_i : G_i \}) = \begin{cases}
\oplus \{ \ell_i : \pi_r(G_i) \} & \text{if}\ r = r'\ \text{(decider)} \\
\& \{ \ell_i : \pi_r(G_i) \} & \text{if}\ r \neq r'\ \text{(observer)}
\end{cases}$$

$$\pi_r(\mu X.\ G) = \begin{cases}
\mu X.\ \pi_r(G) & \text{if}\ \pi_r(G) \neq \text{end} \\
\text{end} & \text{if}\ \pi_r(G) = \text{end}
\end{cases}$$

$$\pi_r(X) = X$$
$$\pi_r(\text{end}) = \text{end}$$

### 3.4 Duality and Safety

For binary session types, duality ensures complementary behavior:

$$\text{dual}(!T\ .\ L) = ?T\ .\ \text{dual}(L)$$
$$\text{dual}(?T\ .\ L) = !T\ .\ \text{dual}(L)$$
$$\text{dual}(\oplus \{ \ell_i : L_i \}) = \& \{ \ell_i : \text{dual}(L_i) \}$$
$$\text{dual}(\& \{ \ell_i : L_i \}) = \oplus \{ \ell_i : \text{dual}(L_i) \}$$
$$\text{dual}(\mu X.\ L) = \mu X.\ \text{dual}(L)$$
$$\text{dual}(X) = X$$
$$\text{dual}(\text{end}) = \text{end}$$

**Property**: If Alice's local type is $L$, then Bob's local type is $\text{dual}(L)$ for their communication to be type-safe.

### 3.5 Session Type Safety Guarantees

The projection process ensures:

1. **Deadlock Freedom**: No circular dependencies in communication
2. **Type Safety**: Messages have correct types at send/receive
3. **Communication Safety**: Every send matches a receive
4. **Progress**: Protocols always advance (no livelocks)
5. **Agreement**: All participants agree on the chosen branch and protocol state (modulo permitted interleavings of independent actions)

### 3.6 Turing Completeness vs Safety Restrictions

The MPST algebra is Turing complete when recursion ($\text{Rec}/\text{Var}$) is unrestricted. However, well-typed programs intentionally restrict expressivity to ensure critical safety properties:

- **Termination**: Protocols that always complete (no infinite loops)
- **Deadlock Freedom**: No circular waiting on communication
- **Progress**: Protocols always advance to next state

Rumpsteak balances expressivity and safety through guarded recursion constructs.

### 3.7 Free Algebra View (Choreography as Initial Object)

You can think of the choreography language as a small set of protocol-building moves:

**Generators:**
- $\text{Send}(r_1, r_2, T, [\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L])$
- $\text{Broadcast}(r, R^*, T, [\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L])$
- $\text{Parallel}(G_1, \ldots, G_n)$
- $\text{Choice}(r, \{\ell_i \mapsto G_i\}_{i \in I})$
- $\text{Rec}(X, G)$ and $\text{Var}(X)$
- $\text{End}$

Taken together, these moves form a "free algebra": the language carries just enough structure to compose protocols, but no extra operational behavior. The effect runtime is the target algebra that gives these moves concrete meaning.

Projection (from a global protocol to each role) followed by interpretation (running it against the effect runtime) yields one canonical way to execute any choreography.

The "free" (initial) property is what keeps this modular. Because the choreographic layer only expresses structure, any effect runtime that respects those composition laws admits exactly one interpretation of a given protocol. This allows swapping or layering handlers without changing choreographies.

The system treats computation and communication symmetrically. A step is the same transform whether it happens locally or across the network. If the sender and receiver are the same role, the projection collapses the step into a local effect call. If they differ, it becomes a message exchange with the same surrounding journal/guard/leak actions. Protocol authors write global transforms, the interpreter decides local versus remote at time of projection.

### 3.9 Algebraic Effects and the Interpreter

Aura treats protocol execution as interpretation over an algebraic effect interface. After projecting a global choreography to each role, a polymorphic interpreter walks the role's AST and dispatches each operation to `AuraEffectSystem` via handlers and middleware. The core actions are exactly the ones defined by the calculus and effect signatures in this document: $\text{merge}$ (facts grow by $\sqcup$), $\text{refine}$ (caps shrink by $\sqcap$), $\text{send}/\text{recv}$ (context-scoped communication), and leakage/budget metering. The interpreter enforces the lattice laws and guard predicates while executing these actions in the order dictated by the session type.

Because the interface is algebraic, there is a single semantics regardless of execution strategy. This enables two interchangeable modes:

- **Static compilation**: choreographies lower to direct effect calls with zero runtime overhead.
- **Dynamic interpretation**: choreographies execute through the runtime interpreter for flexibility and tooling.

Both preserve the same program structure and checks; the choice becomes an implementation detail. This also captures the computation/communication symmetry: a choreographic step describes a typed transform. If the sender and receiver are the same role, projection collapses the step to a local effect invocation. If they differ, the interpreter performs a network send/receive with the same surrounding $\text{merge}/\text{check\_caps}/\text{refine}/\text{record\_leak}$ sequence. Protocol authors reason about transforms, the interpreter decides locality at projection time.

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

$$\text{CvSync}\langle S \rangle := \mu X.\ (A \to B : \text{State}\langle S \rangle\ .\ X) \parallel (B \to A : \text{State}\langle S \rangle\ .\ X)$$

**Δ-CRDT (Delta-based Gossip):**
Δ-CRDTs optimize CvRDTs by transmitting deltas rather than full states.

$$\text{DeltaSync}\langle \Delta \rangle := \mu X.\ (A \to B : \text{DeltaMsg}\langle \Delta \rangle\ .\ X) \parallel (B \to A : \text{DeltaMsg}\langle \Delta \rangle\ .\ X)$$

**CmRDT (Operation-based):**
CmRDTs propagate operations with causal broadcast guarantees.

$$\text{OpBroadcast}\langle \text{Op}, \text{Ctx} \rangle := \mu X.\ (r \triangleright \{\text{issue} : r \to * : \text{OpWithCtx}\langle \text{Op}, \text{Ctx} \rangle\ .\ X,\ \text{idle} : \text{end}\})$$

**Meet-based Constraint Propagation:**
Meet CRDTs handle constraint intersection and capability refinement.

$$\text{ConstraintSync}\langle C \rangle := \mu X.\ (A \to B : \text{ConstraintMsg}\langle C \rangle\ .\ X) \parallel (B \to A : \text{ConstraintMsg}\langle C \rangle\ .\ X)$$

### 4.4 Convergence Properties

**Safety & Convergence:**
- **Session safety**: Projection ensures dual locals, communication safety, and deadlock freedom
- **Cv/Δ convergence**: eventual delivery + semilattice laws $\implies$ states converge to the join of all local updates
- **Cm convergence**: causal delivery + dedup + commutative ops $\implies$ replicas converge modulo permutation of independent ops
- **Meet convergence**: constraint propagation + meet laws $\implies$ capabilities converge to intersection of all constraints

## 5. Information Flow Contract (Privacy + Spam)

### 5.1 Privacy Layers

For any trace $\tau$ of observable messages:

1. **Unlinkability:** $\forall \kappa_1 \neq \kappa_2,\ \tau[\kappa_1 \leftrightarrow \kappa_2] \approx_{\text{ext}} \tau$
2. **Non-amplification:** Information visible to observer class $o$ is monotone in authorized capabilities:
   $$I_o(\tau_1) \leq I_o(\tau_2) \iff C_o(\tau_1) \leq C_o(\tau_2)$$
3. **Leakage Bound:** For each observer $o$, $L(\tau, o) \leq \text{Budget}(o)$.
4. **Flow Budget Soundness (Named):**
   - Charge-Before-Send
   - No-Observable-Without-Charge
   - Deterministic-Replenishment
   - **Convergence**: Within a fixed epoch and after convergence, $\text{spent}_\kappa \leq \min_r \text{limit}_\kappa^r$ across replicas $r$.

### 5.2 Web-of-Trust Model

Let $W = (V, E)$ where vertices are accounts; edges carry relationship contexts and delegation fragments.

- Each edge $(A, B)$ defines a **pairwise context** $\text{RID}_{AB}$ with derived keys
- Delegations are meet-closed elements $d \in \text{Cap}$, scoped to contexts
- The **effective capability** at $A$ is:
  $$\text{Caps}_A = (\text{LocalGrants}_A \sqcap \bigcap_{(A,x) \in E} \text{Delegation}_{x \to A}) \sqcap \text{Policy}_A$$

**WoT invariants:**
- **Compositionality:** Combining multiple delegations uses $\sqcap$ (never widens)
- **Local sovereignty:** $\text{Policy}_A$ is always in the meet; $A$ can only reduce authority further
- **Projection:** For any protocol projection to $A$, guard checks refer to $\text{Caps}_A(\text{ctx})$

### 5.3 Flow Budget Contract

The unified information-flow budget regulates emission rate/volume and observable leakage using the same semilattice laws as capabilities and facts. For any context $\kappa$ and peer $p$:

1. **Charge-Before-Send**: A send or forward is permitted only if a budget charge succeeds first. If charging fails, the step blocks locally and emits no network observable.
2. **No-Observable-Without-Charge**: For any trace $\tau$, there is no event labeled $\text{send}(\kappa, p, \ldots)$ without a preceding successful charge for $(\kappa, p)$ in the same epoch.
3. **Receipt soundness**: A relay accepts a packet only with a valid per-hop $\text{Receipt}$ (context-scoped, epoch-bound, signed) and sufficient local headroom; otherwise it drops locally.
4. **Deterministic replenishment**: $\text{limit}_\kappa$ updates are deterministic functions of journal facts and converge via meet; $\text{spent}_\kappa$ is join-monotone. Upon epoch rotation, $\text{spent}_\kappa$ resets and receipts rebind to the new epoch.
5. **Context scope**: Budget facts and receipts are scoped to $\kappa$; they neither leak nor apply across distinct contexts (non-interference).
6. **Composition with caps**: A transport effect requires both $\text{need}(m) \leq C$ and $\text{headroom}(\kappa, \text{cost}, F, C)$ (see §1.3). Either guard failing blocks the effect.
7. **Convergence bound**: Within a fixed epoch and after convergence, $\text{spent}_\kappa \leq \min_r \text{limit}_\kappa^r$ across replicas $r$.

## 6. Application Model

Every distributed protocol $G$ is defined as a multi-party session type with role projections:

$$G ::= \mu X.\ A \to B : m\langle T \rangle\ [\text{guard}\ \text{need}(m) \leq C_A,\ \text{update}\ F_A \sqcup= \Delta F,\ \text{refine}\ C_B \sqcap= \Delta C];\ X$$

When executed, each role $\rho$ instantiates a handler:

$$\text{handle protocol}(G, \rho)\ \text{with}\ \{\text{on\_send}, \text{on\_recv}, \text{on\_merge}, \text{on\_refine}\}$$

Handlers compose algebraically over $(F, C)$ by distributing operations over semilattice state transitions. This yields an *effect runtime* capable of:

- key-ceremony coordination (threshold signatures)
- gossip and rendezvous (context-isolated send/recv)
- distributed indexing (merge facts, meet constraints)
- garbage collection (join-preserving retractions)

## 7. Interpretation

Under this calculus, we can make the following interpretation:

### The Semilattice Layer

The **join-semilattice (Facts)** captures evidence and observations (trust and information flow). Examples: delegations/attestations, quorum proofs, ceremony transcripts, flow receipts, and monotone $\text{spent}$ counters.

The **meet-semilattice (Capabilities)** captures enforcement limits and constraints (trust and information flow). Examples: local policy, revocations, capability constraints, per-context $\text{limit}$ budgets, leak bounds, and consent gates.

Effective authority and headroom are computed from both lattices:
$$C_{\text{eff}}(F, C) = \text{derive\_caps}(F) \sqcap C \sqcap \text{Policy}$$
$$\text{headroom}(F, C) \text{ uses } \text{limit} \in C \text{ and } \text{spent} \in F, \text{ permitting sends iff } \text{spent} + \text{cost} \leq \text{limit}$$

### The Session-Typed Process Layer

This layer guarantees *communication safety* and *progress*. It projects global types with annotations $[\text{guard}: \Gamma,\ \triangleright \Delta,\ \text{leak}: L]$ into local programs, ensuring deadlock freedom, communication safety, branch agreement, and aligning capability checks, journal updates, and leakage accounting with each send/recv.

### The Effect Handler Layer

The Effect Handler system provides *operational semantics and composability*. It realizes $\text{merge}/\text{refine}/\text{send}/\text{recv}$ as algebraic effects, enforces lattice monotonicity ($\sqcup$ facts, $\sqcap$ caps), guard predicates, and budget/leakage metering, and composes via middleware across crypto, storage, and transport.

### The Privacy Contract

The privacy contract defines *which transitions are observationally equivalent*. Under context isolation and budgeted leakage, traces that differ only by in-context reorderings or by merges/refinements preserving observer-class budgets and effective capabilities are indistinguishable. No cross-context flow occurs without a typed bridge.

Together, these form a *privacy-preserving, capability-checked distributed λ-calculus*.

## See Also

- [Project Overview](000_project_overview.md) - Overall project architecture and goals
- [System Architecture](002_system_architecture.md) - Implementation patterns and system design
- [Information Flow](003_information_flow.md) - Concrete applications and examples
- [Flow Budget System](103_flow_budget_system.md) - Unified budget model for privacy and spam
