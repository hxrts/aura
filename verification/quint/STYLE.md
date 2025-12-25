# Quint Style Guide for Aura Verification

This guide establishes conventions for Quint model development in the Aura verification suite.

## Module Structure

### File Organization

```
verification/quint/
├── STYLE.md                           # This guide
├── README.md                          # Overview and usage
├── protocol_consensus.qnt             # Core consensus spec
├── protocol_consensus_adversary.qnt   # Byzantine behavior models
├── protocol_consensus_liveness.qnt    # Liveness/termination properties
├── protocol_*.qnt                     # Other protocol specs
└── harness_*.qnt                      # Test harnesses
```

### Module Template

Each protocol module should follow this structure:

```quint
// Module Title - Brief Description
//
// Detailed description of what this module models.
//
// == Lean Correspondence ==
// - Module: Aura.Consensus.ModuleName
// - Types: See TYPE DEFINITIONS section
// - Invariants: See INVARIANTS section
//
// == Rust Correspondence ==
// - File: crates/aura-*/src/*.rs
// - Types: TypeName
//
// See: docs/XXX_documentation.md

module protocol_name {
    // ==================== TYPE DEFINITIONS ====================
    // Core domain types with Lean correspondence comments.

    // Lean: Aura.Consensus.Types.TypeName
    type TypeName = ...

    // ==================== STATE VARIABLES ====================
    // Module state tracked during execution.

    var stateVar: Type

    // ==================== CONSTANTS ====================
    // Configuration values.

    pure val CONSTANT_NAME: Type = value

    // ==================== HELPER FUNCTIONS ====================
    // Pure computation helpers.

    pure def helperFunction(...): ReturnType = ...

    // ==================== ACTIONS ====================
    // State transitions.

    action init: bool = all { ... }

    action actionName(...): bool = all { ... }

    // ==================== INVARIANTS ====================
    // Safety properties (must hold in every reachable state).
    // Include Lean correspondence for each invariant.

    // Lean: Aura.Consensus.Module.theorem_name
    val InvariantName = ...

    // ==================== TEMPORAL PROPERTIES ====================
    // LTL properties for liveness and safety over time.

    temporal propertyName = ...

    // ==================== STEP RELATION ====================
    // Nondeterministic action selection for model checking.

    action step = any { ... }
}
```

## Naming Conventions

### Types
- PascalCase for types: `ConsensusId`, `CommitFact`, `WitnessState`
- Use meaningful domain names aligned with Lean types
- Suffix with purpose: `*Phase`, `*State`, `*Proposal`

### Variables
- camelCase for state variables: `instances`, `committedFacts`, `globalWitnesses`
- Descriptive names reflecting domain semantics

### Functions and Actions
- camelCase for functions: `getOrDefaultInstance`, `countMatchingProposals`
- Verb phrases for actions: `startConsensus`, `submitWitnessShare`
- Prefix with `is` or `has` for predicates: `isValid`, `hasProposal`

### Constants
- SCREAMING_SNAKE_CASE: `NONCE_VALIDITY_WINDOW`, `MAX_RETRIES`

### Invariants and Properties
- PascalCase with `Invariant` prefix: `InvariantUniqueCommitPerInstance`
- Descriptive names stating the property

## Documentation

### Module Headers
Every module starts with a comment block explaining:
1. Purpose and scope
2. Key properties to verify
3. Lean correspondence
4. Rust correspondence
5. Documentation reference

### Section Headers
Use ASCII box comments for major sections:

```quint
// ==================== SECTION NAME ====================
// Brief description of section contents.
```

### Lean Correspondence
Document which Lean theorem corresponds to each invariant:

```quint
// Lean: Aura.Consensus.Agreement.agreement
// Statement: Valid commits for same consensus have same result.
val InvariantUniqueCommitPerInstance = ...
```

### Type Correspondence
Link Quint types to Lean structures:

```quint
// Lean: Aura.Consensus.Types.CommitFact
// Rust: crates/aura-consensus/src/consensus/types.rs::CommitFact
type CommitFact = { ... }
```

## Type Definitions

### Sum Types (Variants)
Use pipe syntax for variants:

```quint
type ConsensusPhase =
    | ConsensusPending
    | FastPathActive
    | FallbackActive
    | ConsensusCommitted
    | ConsensusFailed
```

### Record Types
Use braces with typed fields:

```quint
type CommitFact = {
    cid: ConsensusId,
    rid: ResultId,
    prestateHash: PrestateHash,
    signature: ThresholdSignature,
    attesters: Set[AuthorityId]
}
```

### Option Types
Define locally if not imported:

```quint
type Option[a] = Some(a) | None
```

## State Management

### Variable Declaration
Declare all state variables in STATE VARIABLES section:

```quint
var instances: ConsensusId -> ConsensusInstance
var committedFacts: Set[CommitFact]
```

### Action Pattern
Actions should:
1. Start with precondition checks
2. Use `all { }` for conjunctive updates
3. Update all state variables explicitly
4. End with state variable assignments

```quint
action actionName(param: Type): bool = {
    // Local computations
    val computed = ...
    all {
        // Preconditions
        precondition1,
        precondition2,
        // State updates (all variables)
        stateVar1' = newValue1,
        stateVar2' = newValue2
    }
}
```

### Initialization
Always define `init` action that sets all variables:

```quint
action init: bool = all {
    instances' = Map(),
    committedFacts' = Set(),
    // ... all other variables
}
```

## Invariants

### Safety Properties
Express as universal statements over state:

```quint
// Lean: Aura.Consensus.Agreement.unique_commit
val InvariantUniqueCommitPerInstance =
    committedFacts.forall(cf1 =>
        committedFacts.forall(cf2 =>
            cf1.cid == cf2.cid implies cf1.rid == cf2.rid
        )
    )
```

### Threshold Properties
Express threshold requirements explicitly:

```quint
// Lean: Aura.Consensus.Validity.commit_has_threshold
val InvariantCommitRequiresThreshold =
    instances.keys().forall(cid => {
        val inst = instances.get(cid)
        match inst.commitFact {
            | Some(cf) => cf.attesters.size() >= inst.threshold
            | None => true
        }
    })
```

## Temporal Properties

### Liveness
Use `eventually` for progress properties:

```quint
temporal livenessEventualCommit = always(
    precondition implies eventually(goalCondition)
)
```

### Safety Over Time
Use `always` for persistent safety:

```quint
temporal safetyImmutableCommit = always(
    condition implies always(preservedCondition)
)
```

## Step Relations

### Nondeterministic Choice
Use `any { }` for action selection:

```quint
action step = any {
    action1(...),
    action2(...),
    action3(...)
}
```

### Parameter Generation
Use `nondet` for nondeterministic values:

```quint
action step = any {
    nondet cid = oneOf(Set("cns1", "cns2"))
    nondet witness = oneOf(Set("w1", "w2", "w3"))
    submitWitnessShare(cid, witness, ...)
}
```

## Lean/Quint Correspondence Table

| Quint Invariant | Lean Theorem |
|----------------|--------------|
| `InvariantUniqueCommitPerInstance` | `Aura.Consensus.Agreement.agreement` |
| `InvariantCommitRequiresThreshold` | `Aura.Consensus.Validity.commit_has_threshold` |
| `InvariantEquivocatorsExcluded` | `Aura.Consensus.Equivocation.exclusion_correctness` |
| `InvariantSignatureBindsToCommitFact` | `Aura.Consensus.Frost.share_binding` |
| `InvariantByzantineThreshold` | `Aura.Assumptions.byzantine_threshold` |

## Module Dependencies

### Import Pattern
Use qualified imports with aliases:

```quint
import protocol_consensus as core from "protocol_consensus"
```

### Type Access
Use qualified names for imported types:

```quint
core::ConsensusId
core::CommitFact
```

## Validation

### Type Checking
```bash
quint typecheck protocol_consensus.qnt
```

### Model Checking Invariants
```bash
quint run --invariant=InvariantUniqueCommitPerInstance protocol_consensus.qnt
```

### Generate Traces
```bash
quint run --out-itf=trace.itf.json protocol_consensus.qnt
```

## File Checklist

For each protocol module:

- [ ] Header with purpose, Lean correspondence, Rust correspondence
- [ ] TYPE DEFINITIONS section with correspondence comments
- [ ] STATE VARIABLES section
- [ ] CONSTANTS section (if applicable)
- [ ] HELPER FUNCTIONS section
- [ ] ACTIONS section with init
- [ ] INVARIANTS section with Lean correspondence
- [ ] TEMPORAL PROPERTIES section (if applicable)
- [ ] STEP RELATION section
- [ ] All invariants have Lean theorem correspondence
- [ ] Module compiles with `quint typecheck`
