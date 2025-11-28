# Relational Context Invariants

## Context Isolation Invariant

### Invariant Name
`CONTEXT_ISOLATION`

### Description
Information must not flow across relational context boundaries without explicit authorization. Each context maintains a separate journal namespace and state space.

### Enforcement Locus

1. **Namespace Separation**:
   - Module: `aura-core/src/types/context.rs`
   - Type: `ContextId` - Opaque identifier for relational contexts
   - Function: `JournalNamespace::from_context()` - Maps contexts to isolated namespaces

2. **Journal Isolation**:
   - Module: `aura-journal/src/namespace.rs`
   - Function: `validate_namespace_access()` - Ensures facts stay in correct namespace
   - Property: Facts from context A cannot appear in context B's journal

3. **State Reduction Boundaries**:
   - Module: `aura-journal/src/reduce/relational.rs`
   - Function: `reduce_context_state()` - Reduces only facts from single context
   - Property: Reduction never reads across context boundaries

4. **Transport Scoping**:
   - Module: `aura-transport/src/channel/context_scoped.rs`
   - Function: `SecureChannel::bind_to_context()` - Channels bound to single context
   - Property: Messages cannot leak between contexts via channels

### Failure Mode

**Observable Consequences**:
1. **Privacy Violation**: Information from one relationship visible in another
2. **Authority Confusion**: Actions in context A affect context B
3. **Capability Leakage**: Permissions granted in one context used in another

**Attack Scenarios**:
- Malicious peer attempts cross-context replay attack
- Bug allows facts to be written to wrong namespace  
- Reducer accidentally reads from multiple contexts

### Detection Method

1. **Namespace Validation**:
   ```rust
   #[test]
   fn test_namespace_isolation() {
       let ctx_a = ContextId::new();
       let ctx_b = ContextId::new();
       
       // Facts for context A must fail validation in context B
       let fact_a = Fact::new(ctx_a, ...);
       assert!(validate_for_context(fact_a, ctx_b).is_err());
   }
   ```

2. **Simulator Scenarios**:
   - Test: `test_context_isolation_under_attack()`
   - Scenario: Malicious peer sends facts for wrong context
   - Expected: Facts rejected at validation, no state change

3. **arch-check Patterns**:
   ```bash
   # Flag cross-context access attempts
   grep -r "ContextId" --include="*.rs" | \
     grep -E "(get|read|write).*different.*context"
   ```

### Related Invariants
- `CAPABILITY_CONTEXT_BINDING`: Capabilities are scoped to contexts
- `CHANNEL_CONTEXT_AFFINITY`: Channels belong to single context
- `NAMESPACE_IMMUTABILITY`: Context→namespace mapping never changes

### Implementation Notes

Context isolation is enforced at multiple layers:

```rust
// CORRECT: Context-scoped operation
async fn append_relational_fact(
    ctx: &EffectContext,
    context_id: ContextId,
    fact: RelationalFact,
) -> Result<()> {
    // Validate context match
    if fact.context_id() != context_id {
        return Err(AuraError::ContextMismatch);
    }
    
    // Get isolated namespace
    let namespace = JournalNamespace::from_context(context_id);
    
    // Append only to correct namespace
    journal.append_fact(namespace, fact).await
}

// WRONG: Cross-context access
async fn bad_cross_context_read(
    contexts: Vec<ContextId>
) -> State {
    let mut combined = State::default();
    for ctx in contexts {
        // This violates isolation!
        combined.merge(read_context(ctx).await);
    }
    combined
}
```

### Verification

Isolation tests:
```bash
cargo test -p aura-core context_isolation
cargo test -p aura-journal namespace_separation  
```

See also: [docs/103_relational_contexts.md](../../../../docs/103_relational_contexts.md) for architectural details.