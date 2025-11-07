# Meet Semi-Lattice Implementation Proposal

## Abstract

This document proposes extending Aura's harmonized CRDT architecture to support meet semi-lattices as the algebraic dual to the existing join semi-lattice implementation. While CRDTs traditionally rely on join semi-lattices for conflict-free merging, meet semi-lattices enable dual operations such as constraint satisfaction, capability restriction, and security policy intersection.

## Mathematical Foundation

### Join vs. Meet Semi-Lattices

A **join semi-lattice** (∨-semi-lattice) is a partially ordered set where every pair of elements has a least upper bound (supremum):
- Operation: `a ∨ b` (join)
- Property: `a ≤ (a ∨ b)` and `b ≤ (a ∨ b)`
- CRDT semantics: Accumulative, grows monotonically

A **meet semi-lattice** (∧-semi-lattice) is a partially ordered set where every pair of elements has a greatest lower bound (infimum):
- Operation: `a ∧ b` (meet)
- Property: `(a ∧ b) ≤ a` and `(a ∧ b) ≤ b`
- CRDT semantics: Restrictive, constrains monotonically

### Dual Relationship

For any join semi-lattice `(S, ≤, ∨)`, there exists a dual meet semi-lattice `(S, ≥, ∧)` where:
- The order is reversed: `a ≤ b` in join corresponds to `a ≥ b` in meet
- Join becomes meet: `a ∨ b` in join corresponds to `a ∧ b` in meet
- Top element `⊤` in join becomes bottom element `⊥` in meet

## Use Cases in Aura

### 1. Capability Restriction
When capabilities propagate through the system, meet operations ensure that derived capabilities never exceed the intersection of their sources:
```
device_caps ∧ session_caps ∧ resource_caps = effective_caps
```

### 2. Security Policy Intersection
Multiple security policies can be combined by taking their meet, ensuring the result is no less restrictive than any component policy:
```
policy₁ ∧ policy₂ ∧ policy₃ = combined_policy
```

### 3. Consensus Constraints
In threshold protocols, meet operations can represent the intersection of participant constraints, ensuring global consistency:
```
participant_constraints₁ ∧ ... ∧ participant_constraintsₙ = consensus_constraints
```

### 4. Temporal Access Control
Time-based access windows can be intersected to find valid access periods:
```
validity_window₁ ∧ validity_window₂ = intersection_window
```

## Implementation Architecture

### Foundation Layer

**Location**: `aura-types/src/semilattice/semantic_traits.rs`

Extend the existing foundation with meet semi-lattice traits:

```rust
/// Meet semi-lattice with greatest lower bound operation
pub trait MeetSemiLattice: Clone {
    /// Meet operation (greatest lower bound)
    fn meet(&self, other: &Self) -> Self;
}

/// Top element for meet semi-lattices (most permissive state)
pub trait Top {
    /// Return the top element (⊤)
    fn top() -> Self;
}

/// Meet-based CRDT state combining MeetSemiLattice and Top
pub trait MvState: MeetSemiLattice + Top {}
```

**Algebraic Laws**: All implementations must satisfy:
- **Commutativity**: `a ∧ b = b ∧ a`
- **Associativity**: `(a ∧ b) ∧ c = a ∧ (b ∧ c)`
- **Idempotence**: `a ∧ a = a`
- **Identity**: `a ∧ ⊤ = a`

### Message Types

**Location**: `aura-types/src/semilattice/message_types.rs`

Extend message types for meet semi-lattice communication:

```rust
/// Meet-based state synchronization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetStateMsg<S> {
    pub payload: S,
    pub kind: MsgKind,
    pub monotonic_counter: u64, // Ensures proper ordering
}

/// Meet-based constraint message for policy intersection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintMsg<C> {
    pub constraint: C,
    pub scope: ConstraintScope,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConstraintScope {
    Global,
    Session(SessionId),
    Device(DeviceId),
    Resource(String),
}
```

### Effect Handler Layer

**Location**: `aura-protocol/src/effects/semilattice/mv_handler.rs`

Create meet-based effect handlers:

```rust
/// Meet-based CRDT effect handler enforcing meet semi-lattice laws
pub struct MvHandler<S: MvState> {
    pub state: S,
    constraint_history: Vec<ConstraintEvent<S>>,
}

impl<S: MvState> MvHandler<S> {
    pub fn new() -> Self {
        Self {
            state: S::top(), // Start with most permissive state
            constraint_history: Vec::new(),
        }
    }

    /// Apply constraint through meet operation
    pub fn on_constraint(&mut self, constraint: S) {
        let previous = self.state.clone();
        self.state = self.state.meet(&constraint);

        // Record constraint application
        self.constraint_history.push(ConstraintEvent {
            previous,
            constraint,
            result: self.state.clone(),
            timestamp: current_timestamp(),
        });
    }

    /// Verify constraint satisfaction
    pub fn satisfies_constraint(&self, constraint: &S) -> bool {
        self.state.meet(constraint) == self.state
    }
}
```

### Choreographic Protocol Layer

**Location**: `aura-choreography/src/semilattice/meet_protocols.rs`

Implement session-type protocols for meet semi-lattice synchronization:

```rust
use rumpsteak_choreography::choreography;

choreography! {
    MvConstraint {
        roles: Enforcer[N]

        protocol ConstraintPropagation {
            // Each enforcer proposes constraints
            loop (count: N) {
                Enforcer[i] -> Enforcer[*]: ConstraintMsg
            }

            // Compute intersection of all constraints
            loop (count: N) {
                Enforcer[i].local_meet_computation()
            }

            // Verify consistency
            loop (count: N) {
                Enforcer[i] -> Enforcer[*]: ConsistencyProof
            }
        }

        call ConstraintPropagation
    }
}

/// Execute meet-based constraint synchronization
pub async fn execute_constraint_sync<S: MvState + Send + Sync>(
    adapter: &mut AuraHandlerAdapter,
    handler: &mut MvHandler<S>,
    enforcers: Vec<DeviceId>,
    my_role: usize,
    constraint: S,
) -> Result<(), ChoreographyError> {
    // Implementation coordinates constraint propagation
    // ensuring global consistency through meet operations
    unimplemented!("Requires choreographic runtime completion")
}
```

### Application Layer Integration

**Location**: `aura-journal/src/semilattice/meet_types.rs`

Domain-specific meet semi-lattice CRDTs:

```rust
/// Capability set with meet-based restriction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub read_permissions: BTreeSet<String>,
    pub write_permissions: BTreeSet<String>,
    pub admin_permissions: BTreeSet<String>,
    pub expiry_time: Option<u64>,
}

impl MeetSemiLattice for CapabilitySet {
    fn meet(&self, other: &Self) -> Self {
        Self {
            // Intersection of permissions (more restrictive)
            read_permissions: self.read_permissions
                .intersection(&other.read_permissions)
                .cloned()
                .collect(),
            write_permissions: self.write_permissions
                .intersection(&other.write_permissions)
                .cloned()
                .collect(),
            admin_permissions: self.admin_permissions
                .intersection(&other.admin_permissions)
                .cloned()
                .collect(),
            // Earlier expiry time (more restrictive)
            expiry_time: match (self.expiry_time, other.expiry_time) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
        }
    }
}

impl Top for CapabilitySet {
    fn top() -> Self {
        // Most permissive state: all permissions, no expiry
        Self {
            read_permissions: ["*".to_string()].into_iter().collect(),
            write_permissions: ["*".to_string()].into_iter().collect(),
            admin_permissions: ["*".to_string()].into_iter().collect(),
            expiry_time: None,
        }
    }
}

impl MvState for CapabilitySet {}

/// Time window with meet-based intersection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub start: u64,
    pub end: u64,
}

impl MeetSemiLattice for TimeWindow {
    fn meet(&self, other: &Self) -> Self {
        Self {
            start: self.start.max(other.start), // Latest start
            end: self.end.min(other.end),       // Earliest end
        }
    }
}

impl Top for TimeWindow {
    fn top() -> Self {
        Self {
            start: 0,
            end: u64::MAX,
        }
    }
}

impl MvState for TimeWindow {}
```

## Duality and Composition

### Join-Meet Duality

The implementation supports systems that use both join and meet operations through dual mappings:

```rust
/// Convert join semi-lattice to its meet dual
pub trait JoinToDual<T> {
    fn to_dual(&self) -> T;
}

/// Convert meet semi-lattice to its join dual
pub trait MeetToDual<T> {
    fn to_dual(&self) -> T;
}
```

### Galois Connections

For advanced applications, Galois connections can be established between join and meet semi-lattices, enabling systematic conversion between accumulative and restrictive semantics:

```
F: (Join, ≤, ∨) → (Meet, ≥, ∧)
G: (Meet, ≥, ∧) → (Join, ≤, ∨)
```

Where `F ⊣ G` forms an adjunction with `F(x) ≤ y ↔ x ≤ G(y)`.

## Security Considerations

### Constraint Verification

Meet semi-lattices in security contexts require additional verification:

1. **Constraint Authenticity**: All constraint applications must be cryptographically signed
2. **Constraint Ordering**: Temporal ordering prevents constraint rollback attacks
3. **Constraint Bounds**: Lower bounds prevent over-restriction denial of service

### Privacy Protection

Meet operations on capability sets must preserve privacy:
- Use zero-knowledge proofs for capability intersection without revelation
- Employ homomorphic techniques for private constraint evaluation
- Implement differential privacy for constraint aggregation

## Testing and Validation

### Property-Based Testing

**Location**: `aura-types/src/semilattice/tests/meet_properties.rs`

Validate algebraic laws through property-based testing:

```rust
proptest! {
    #[test]
    fn meet_commutativity(a: CapabilitySet, b: CapabilitySet) {
        assert_eq!(a.meet(&b), b.meet(&a));
    }

    #[test]
    fn meet_associativity(a: CapabilitySet, b: CapabilitySet, c: CapabilitySet) {
        assert_eq!(a.meet(&b).meet(&c), a.meet(&b.meet(&c)));
    }

    #[test]
    fn meet_idempotence(a: CapabilitySet) {
        assert_eq!(a.meet(&a), a);
    }

    #[test]
    fn meet_identity(a: CapabilitySet) {
        assert_eq!(a.meet(&CapabilitySet::top()), a);
    }
}
```

### Integration Testing

**Location**: `aura-journal/src/semilattice/tests/meet_integration.rs`

End-to-end validation of meet semi-lattice protocols in realistic scenarios including:
- Capability intersection workflows
- Time window coordination scenarios
- Security policy composition patterns
- Consensus constraint satisfaction
- Property-based testing of algebraic laws

## Conclusion

Meet semi-lattices provide the algebraic dual to Aura's join semi-lattice foundation, enabling systematic handling of constraints, capabilities, and security policies. This system harmonizes with the existing architecture while providing new capabilities for distributed constraint satisfaction and policy intersection.

The dual nature of join and meet operations creates a complete algebraic foundation for distributed systems, supporting both accumulative (join) and restrictive (meet) semantics within a unified semilattice framework. This enables Aura to handle the full spectrum of distributed coordination patterns, from growth-oriented state synchronization to constraint-based capability restriction.
