# Multi-Party Session Type Algebra

This document describes the precise algebraic structure of Aura's choreographic programming system, which forms the free algebra interface between choreographies and the effects system.

## Overview

Aura uses `rumpsteak-aura` for choreographic programming with multi-party session types. The system has two layers:

1. **Session Type Algebra** (choreographic structure) - The protocol composition layer
2. **Effects Algebra** (primitive operations) - The execution substrate (see `400_effect_system.md`)

The session type algebra is the primary compositional interface. It describes distributed protocols algebraically, which are then executed via the effects system.

## The Two-Level Algebra

```
Session Type Algebra (Global Protocol)
    ↓ projection
Local Session Types (Per-Role Protocols)
    ↓ execution via
Effect Algebra (CryptoEffects, NetworkEffects, etc.)
    ↓ interpretation by
Handler Implementations
```

## Formal Type System

### Global Type Grammar (G)

The global choreography type describes the entire protocol from a bird's-eye view:

```
G ::= r₁ → r₂ : T . G                   // Point-to-point send
    | r → * : T . G                     // Broadcast (one-to-many)
    | G ∥ G                             // Parallel composition
    | r ⊳ { ℓᵢ : Gᵢ }ᵢ∈I               // Choice (role r decides among branches)
    | μX . G                             // Recursion
    | X                                  // Recursion variable
    | end                                // Termination

T ::= Unit | Bool | Int | String | ...  // Message types
r ::= Role identifiers (Alice, Bob, ...)
ℓ ::= Label identifiers (accept, reject, ...)
```

**Conventions**:
- `r₁ → r₂ : T . G` means "role r₁ sends message of type T to role r₂, then continue with G"
- `r → * : T . G` means "role r broadcasts message of type T to all other roles, then continue with G"
- `G₁ ∥ G₂` means "execute G₁ and G₂ concurrently"
- `r ⊳ { ℓᵢ : Gᵢ }` means "role r decides which branch ℓᵢ to take, affecting all participants"
- `μX . G` binds recursion variable X in G

### Local Type Grammar (L)

After projection, each role executes a local session type (binary protocol):

```
L ::= ! T . L                            // Send (output)
    | ? T . L                            // Receive (input)
    | ⊕ { ℓᵢ : Lᵢ }ᵢ∈I                   // Internal choice (select)
    | & { ℓᵢ : Lᵢ }ᵢ∈I                   // External choice (branch)
    | μX . L                             // Recursion
    | X                                  // Recursion variable
    | end                                // Termination
```

**Conventions**:
- `! T . L` means "send message of type T, then continue with L"
- `? T . L` means "receive message of type T, then continue with L"
- `⊕ { ℓᵢ : Lᵢ }` means "select label ℓᵢ (internal choice), then continue with Lᵢ"
- `& { ℓᵢ : Lᵢ }` means "branch on received label ℓᵢ (external choice), then continue with Lᵢ"
- `μX . L` binds recursion variable X in L

The local grammar L is the binary session type grammar. After projection from global G, each role's protocol is a standard binary session type that interacts with other roles pairwise.

### Projection Function (π)

The projection function `πᵣ(G)` extracts role r's local view from global choreography G:

```
πᵣ(r₁ → r₂ : T . G) =
    ! T . πᵣ(G)           if r = r₁
    ? T . πᵣ(G)           if r = r₂
    πᵣ(G)                 if r ∉ {r₁, r₂}

πᵣ(s → * : T . G) =
    ! T . πᵣ(G)           if r = s
    ? T . πᵣ(G)           if r ≠ s

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

**Merge Operator (⊙)**:
The merge operator combines two local types when a role participates in parallel branches:

```
L₁ ⊙ L₂ = sequential interleaving of L₁ and L₂
          (order-preserving, conflict-free)

Conflicts detected:
- Multiple sends to same role
- Multiple receives from same role
- Violates → ProjectionError::InconsistentParallel
```

### Duality

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

## Multi-Party Session Type Algebra

Rumpsteak defines two representations of the session type algebra:

### 1. Global Protocol Algebra (`Protocol` AST)

The global choreography as an abstract syntax tree (implemented in `crates/aura-types/src/sessions.rs`):

```rust
enum Protocol {
    // Sequential composition (implicit via continuation)
    Send {
        from: Role,
        to: Role,
        message: MessageType,
        continuation: Box<Protocol>,  // Sequential composition
    },

    // Broadcast (one-to-all)
    Broadcast {
        from: Role,
        to_all: Vec<Role>,
        message: MessageType,
        continuation: Box<Protocol>,
    },

    // Parallel composition
    Parallel {
        protocols: Vec<Protocol>,  // Concurrent execution
    },

    // Choice (branching)
    Choice {
        role: Role,              // Deciding role
        branches: Vec<Branch>,   // Possible paths
    },

    // Recursion
    Rec {
        label: String,
        body: Box<Protocol>,
    },
    Var(String),  // Reference to recursive point

    // Termination
    End,
}
```

### 2. Effect Algebra (`Program` Free Algebra)

The effect-based representation (first-class programs, implemented in `crates/aura-types/src/effects/choreographic.rs`):

```rust
enum Effect<R, M> {
    // Communication primitives
    Send { to: R, msg: M },
    Recv { from: R, msg_type: &'static str },

    // Choice
    Choose { at: R, label: Label },        // Internal choice
    Offer { from: R },                      // External choice
    Branch {                                // Branching on choice
        choosing_role: R,
        branches: Vec<(Label, Program<R, M>)>,
    },

    // Parallel composition
    Parallel { programs: Vec<Program<R, M>> },

    // Control flow
    Loop {
        iterations: Option<usize>,
        body: Program<R, M>,
    },
    Timeout {
        role: R,
        duration: Duration,
        body: Program<R, M>,
    },

    // Termination
    End,
}

// Program is a sequence of effects with sequential composition
type Program<R, M> = Vec<Effect<R, M>>;
```

## Algebraic Operators

### Sequential Composition (`>>`)

**Global Protocol**: Implicit via `continuation` fields
```rust
Protocol::Send {
    from: Alice,
    to: Bob,
    message: Request,
    continuation: Protocol::Send {  // Sequential
        from: Bob,
        to: Alice,
        message: Response,
        continuation: Protocol::End,
    }
}
```

**Program Algebra**: Explicit via `then()`
```rust
program_a.then(program_b)  // Sequential composition
```

**Properties**:
- Associative: `(a >> b) >> c = a >> (b >> c)`
- Identity: `End >> a = a >> End = a`

### Parallel Composition (`||`)

**Global Protocol**:
```rust
Protocol::Parallel {
    protocols: vec![protocol_a, protocol_b, protocol_c]
}
```

**Program Algebra**:
```rust
Effect::Parallel {
    programs: vec![prog_a, prog_b, prog_c]
}
```

**Properties**:
- Commutative: `a || b = b || a`
- Associative: `(a || b) || c = a || (b || c)`
- Deadlock-free (enforced by projection)

### Choice (`+`)

**Global Protocol**:
```rust
Protocol::Choice {
    role: Alice,
    branches: vec![
        Branch {
            label: "accept",
            guard: None,
            protocol: accept_protocol,
        },
        Branch {
            label: "reject",
            guard: None,
            protocol: reject_protocol,
        }
    ]
}
```

**Program Algebra**:
```rust
// Internal choice (role decides)
Effect::Choose { at: Alice, label: "accept" }

// External choice (role observes)
Effect::Offer { from: Alice }

// Branching handler
Effect::Branch {
    choosing_role: Alice,
    branches: vec![
        ("accept", accept_program),
        ("reject", reject_program),
    ]
}
```

**Properties**:
- Deterministic branching (guards evaluated locally)
- Exhaustive (all roles agree on branches)

### Communication Primitives

**Point-to-Point Send**:
```
Alice -> Bob: Request
```

**Broadcast**:
```
Leader ->* : Announcement
```

Expanded to individual sends:
```rust
Protocol::Send { from: Leader, to: Follower1, ... }
Protocol::Send { from: Leader, to: Follower2, ... }
// etc.
```

### Recursion (`μ`)

**Global Protocol**:
```rust
Protocol::Rec {
    label: "loop",
    body: Protocol::Send {
        from: Server,
        to: Client,
        message: Data,
        continuation: Protocol::Var("loop"),  // Recursive call
    }
}
```

**Properties**:
- Guarded recursion (ensures progress)
- Termination checked by type system

#### Turing Completeness vs Safety Restrictions

The MPST algebra described here is Turing complete when recursion (`Rec`/`Var`) is unrestricted. Unrestricted recursion allows expressing any computable protocol, making the algebraic structure as powerful as a Turing machine.

However, well-typed programs in practice intentionally restrict expressivity to ensure critical safety properties:

- **Termination**: Protocols that always complete (no infinite loops)
- **Deadlock Freedom**: No circular waiting on communication
- **Progress**: Protocols always advance to next state

Rumpsteak balances expressivity and safety through guarded recursion constructs:

```rust
// Unrestricted recursion (Turing complete)
Protocol::Rec { label, body: Protocol::Var(label) }

// Guarded recursion with termination guarantee
Loop {
    iterations: Some(n),  // Fixed number of repetitions
    body: protocol,
}

Loop {
    iterations: None,  // Continuation decided by protocol-level choice/guards
    body: protocol,
}
```

This design philosophy matches Aura's overall approach: maximum safety and correctness guarantees while maintaining sufficient expressivity for real-world distributed protocols.

## Local Projection Rules

The global `Protocol` is projected to `LocalType` for each role:

### Send Projection

```rust
Global: Alice -> Bob: Message

Project to Alice: LocalType::Send {
    to: Bob,
    message: Message,
    continuation: π_Alice(continuation)
}

Project to Bob: LocalType::Receive {
    from: Alice,
    message: Message,
    continuation: π_Bob(continuation)
}

Project to Charlie: π_Charlie(continuation)  // Skip
```

### Choice Projection

```rust
Global: Choice { role: Alice, branches: [...] }

Project to Alice: LocalType::Select { to: recipient, branches }  // internal choice (⊕)
// If the choice never leaves Alice, the projection simply continues with π_Alice of the selected branch.

Project to others: LocalType::Branch {
    from: Alice,
    branches: [(label, π_role(branch_protocol))]
}
```

### Parallel Projection

```rust
Global: Parallel { protocols: [p1, p2, p3] }

Project to role:
  - If role in 0 branches: LocalType::End
  - If role in 1 branch: π_role(that_branch)
  - If role in multiple branches:
      - Check for conflicts (multiple sends/recvs to/from same role)
      - If safe: interleave operations sequentially
      - If conflicts: ProjectionError::InconsistentParallel
```

### Recursion Projection

```
Global: Rec { label, body }

Project to role:
  - If π_role(body) = End: LocalType::End
  - Otherwise: LocalType::Rec {
      label,
      body: π_role(body)
    }
```

## Session Type Safety Guarantees

The projection process ensures:

1. **Deadlock Freedom**: No circular dependencies in communication
2. **Type Safety**: Messages have correct types at send/receive
3. **Communication Safety**: Every send matches a receive
4. **Progress**: Protocols always advance (no livelocks)
5. **Determinism**: All participants agree on protocol state

### Conflict Detection

During parallel projection, conflicts are detected:

```rust
// CONFLICT: Multiple sends to same role
Parallel {
    protocols: [
        Alice -> Bob: Msg1,
        Alice -> Bob: Msg2,  // Error: concurrent sends to Bob
    ]
}

// CONFLICT: Multiple receives from same role
Parallel {
    protocols: [
        Alice -> Bob: Msg1,
        Charlie -> Bob: Msg2,  // Error: Bob has concurrent receives
    ]
}

// SAFE: Different pairs
Parallel {
    protocols: [
        Alice -> Bob: Msg1,
        Charlie -> Dave: Msg2,  // OK: disjoint pairs
    ]
}
```

## The Free Algebra Property

The session type algebra is *free* because:

1. **Polymorphic over execution**: Same choreography runs with different effect handlers
2. **Compositional**: Complex protocols built from simple primitives
3. **Separation of concerns**: Protocol structure independent of implementation
4. **Multiple interpretations**:
   - Production: Real network, crypto, storage
   - Testing: Mock handlers, deterministic execution
   - Simulation: Model-checked, time-travel debugging

### Example: DKD Protocol Algebra

```rust
// Global choreographic structure
type DkdChoreography<N> =
    CommitPhase<N>            // All commit to random values
    >> GatherPhase<N>         // Synchronization point
    >> RevealPhase<N>         // All reveal values
    >> VerifyPhase<N>         // Check consistency
    >> DerivePhase<Context>;  // Compute key material

// Each phase has session type structure
type CommitPhase<N> =
    Parallel<N, λi.                   // For each participant i
        LocalCompute(i, commitment)   // Compute commitment
        >> Broadcast(i, commitment)>; // Broadcast to all

type GatherPhase<N> =
    Gather<N, Commitment>;    // Collect all commitments

type RevealPhase<N> =
    Parallel<N, λi.
        Broadcast(i, reveal)>;

type VerifyPhase<N> =
    LocalCompute(verify_all); // Each verifies independently

type DerivePhase<C> =
    LocalCompute(derive_key); // Each derives same key
```

This choreography executes via:
```rust
// Effects layer (from 400_effect_system.md)
async fn commit_phase<C: CryptoEffects, R: RandomEffects>(
    crypto: &C,
    random: &R,
) -> Commitment {
    let nonce = random.random_bytes(32);
    let value = compute_value();
    let hash = crypto.blake3_hash(&serialize(value, nonce));
    Commitment { hash, epoch }
}
```

## Choreography DSL Syntax

Point-to-point communication:
```rust
Alice -> Bob: Request
Bob -> Alice: Response
```

Broadcast:
```rust
Leader ->* : Announcement
```

Choice:
```rust
choice Alice {
    accept {
        Alice -> Bob: Accept
    }
    reject {
        Alice -> Bob: Reject
    }
}
```

Parallel:
```rust
parallel {
    Alice -> Bob: Data1
    Charlie -> Dave: Data2
}
```

Recursion:
```rust
rec loop {
    Server -> Client: Data
    continue loop
}
```

## Relation to π-Calculus

The session type algebra is founded on π-calculus:

- **Processes**: Roles executing local protocols
- **Channels**: Typed communication links between roles
- **Parallel composition**: `P | Q`
- **Sequential composition**: `P.Q`
- **Choice**: `P + Q`
- **Recursion**: `μX.P`

Key difference: Session types enforce linear channel usage. Each channel used exactly once per session, preventing races and deadlocks.

## Implementation Locations

All session type algebra types are implemented in the `aura-types` crate to ensure they are available foundation-wide:

### Core Algebra Types

- **`Protocol` enum** (Global Protocol AST): `crates/aura-types/src/sessions.rs`
  - Send, Broadcast, Parallel, Choice, Rec, Var, End variants
  - Sequential composition via continuation fields
  - Complete global choreography representation

- **`Effect<R, M>` enum** (Effect-Based Programs): `crates/aura-types/src/effects/choreographic.rs`
  - Send, Recv, Choose, Offer, Branch, Parallel, Loop, Timeout, End variants
  - Generic over Role and Message types
  - Free algebra for sequential composition

- **`Program<R, M>` type**: `crates/aura-types/src/effects/choreographic.rs`
  - Defined as `Vec<Effect<R, M>>` for sequential composition
  - Program combinators (then, parallel, choice)

### Supporting Types

- **`MessageType` enum**: `crates/aura-types/src/identifiers.rs`
  - Unit, Bool, Int, String, Bytes, Custom variants
  - Wire protocol serialization support

- **`Label` and `Branch` types**: `crates/aura-types/src/sessions.rs`
  - Label with string identifier and optional guards
  - Branch with label, guard expression, and Protocol continuation

### Projection and Safety

- **Projection Functions**: `crates/aura-types/src/sessions.rs`
  - Global-to-local projection function `πᵣ(G)`
  - Local type duality function for session type safety
  - Conflict detection for parallel composition

### Import Paths

In your code, import these types with:

```rust
use aura_types::sessions::{Protocol, Label, Branch};
use aura_types::effects::choreographic::{Effect, Program};
use aura_types::identifiers::MessageType;
```

## References

- "A Very Gentle Introduction to Multiparty Session Types"
- "Precise Subtyping for Asynchronous Multiparty Sessions"
- Rumpsteak-Aura: https://github.com/hxrts/rumpsteak-aura

## See Also

- `400_effect_system.md` - The effects algebra execution layer
- `000_overview.md` - Overall architecture and crate structure
