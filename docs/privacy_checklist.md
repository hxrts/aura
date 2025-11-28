# Privacy-by-Design Checklist

This checklist helps developers ensure that new transport and communication code follows Aura's privacy-by-design principles.

## Pre-Development

Before writing new transport-related code, review:

- [ ] [Information Flow Contract](003_information_flow_contract.md) - Privacy guarantees and leakage budgets
- [ ] [Transport and Information Flow](108_transport_and_information_flow.md) - Privacy patterns and guard chain integration
- [ ] [Context Isolation](103_relational_contexts.md) - Relationship and context boundaries
- [ ] `crates/aura-transport/src/` - Reference implementation with privacy-by-design

## Design Phase

### Message Envelope Design

When designing message formats:

- [ ] **Context Scoping**: Every message has explicit `ContextId` or `RelationshipId`
- [ ] **Privacy Level**: Message specifies privacy level (`Clear`, `Blinded`, `RelationshipScoped`)
- [ ] **Minimal Headers**: Only essential routing metadata in frame headers
- [ ] **Capability Blinding**: Capability requirements blinded before network transmission
- [ ] **Fixed Sizes**: Consider padding to prevent size-based correlation

### Peer Selection

When implementing peer selection:

- [ ] **Criteria Blinding**: Selection criteria never exposed in clear text
- [ ] **Privacy-Preserving Scoring**: Scoring algorithm doesn't leak weights or thresholds
- [ ] **No Rejection Logging**: Rejected candidates not logged or exposed
- [ ] **Generic Reasons**: Selection reasons use categories, not specific requirements
- [ ] **Relationship Scoping**: Selection restricted to appropriate relationship context

### Connection Management

When managing connections:

- [ ] **Context Binding**: One connection per `(ContextId, peer)` pair
- [ ] **No Cross-Context**: Connection state never shared between contexts
- [ ] **Epoch Awareness**: Connections torn down on context epoch changes
- [ ] **Re-keying**: Fresh key derivation after epoch rotation
- [ ] **State Isolation**: No correlation between connections in different contexts

## Implementation Phase

### Code Review

Before submitting code:

- [ ] **No Plaintext Capabilities**: Capability names never sent unencrypted
- [ ] **Generic Errors**: Error messages don't reveal relationship structure
- [ ] **Blinded Hints**: All capability/requirement hints blinded
- [ ] **Budget Compliance**: All sends go through FlowGuard
- [ ] **Leakage Tracking**: Metadata exposure accounted in LeakageTracker

### Privacy Pitfalls

Verify you are NOT doing these:

- [ ] ❌ Logging detailed capability requirements
- [ ] ❌ Exposing relationship membership in errors or logs
- [ ] ❌ Reusing connection IDs across contexts
- [ ] ❌ Sending capability names in clear text
- [ ] ❌ Creating timing side channels (e.g., different failure modes)
- [ ] ❌ Logging rejected peers or selection scores
- [ ] ❌ Correlating message sizes with content types
- [ ] ❌ Revealing context membership through observable behavior

### Privacy Best Practices

Verify you ARE doing these:

- [ ] ✅ Using `Envelope::new_scoped()` for relationship messages
- [ ] ✅ Generic error messages ("authorization failed" not "missing: admin")
- [ ] ✅ Padding messages to fixed sizes where possible
- [ ] ✅ Rotating connection identifiers on epoch changes
- [ ] ✅ Blinding all capability hints before transmission
- [ ] ✅ Privacy-preserving peer selection scoring
- [ ] ✅ Context isolation in all state management
- [ ] ✅ No observable behavior differences between privacy levels

## Testing Phase

### Unit Tests

Add tests for:

- [ ] **Context Isolation**: Messages in context A not visible in context B
- [ ] **Metadata Minimization**: Only essential headers present
- [ ] **Selection Privacy**: Peer selection doesn't leak criteria
- [ ] **Re-keying**: Epoch changes trigger connection teardown
- [ ] **No Side Channels**: Timing/size/errors don't leak information

### Integration Tests

Verify:

- [ ] **Guard Chain Integration**: Sends go through CapGuard → FlowGuard → LeakageTracker → JournalCoupler
- [ ] **Budget Enforcement**: Over-budget sends blocked before network transmission
- [ ] **Unauthorized Sends**: Failed authorization produces no network traffic
- [ ] **Context Boundaries**: Cross-context sends rejected
- [ ] **Receipt Generation**: FlowBudget receipts created on successful sends

### Privacy Property Tests

Test privacy properties:

- [ ] **Unlinkability**: Messages in different contexts computationally indistinguishable
- [ ] **Metadata Leakage**: Leakage within budget bounds
- [ ] **Observability**: No observable difference between authorization failure modes
- [ ] **Correlation Resistance**: Cannot correlate messages across contexts
- [ ] **Forward Secrecy**: Epoch rotation invalidates old keys

See `crates/aura-transport/src/types/tests.rs` for example tests.

## Documentation Phase

### Code Documentation

Document:

- [ ] **Privacy Level**: Specify privacy level of each message type
- [ ] **Context Scoping**: Explain context boundaries and isolation
- [ ] **Metadata Exposure**: Document what metadata is exposed and why
- [ ] **Leakage Budget**: Note leakage cost of each operation
- [ ] **Failure Modes**: Explain failure modes and side-channel prevention

### Architecture Documentation

Update relevant docs:

- [ ] [Transport and Information Flow](108_transport_and_information_flow.md) - If adding new patterns
- [ ] [Information Flow Contract](003_information_flow_contract.md) - If changing leakage accounting
- [ ] [Authorization](109_authorization.md) - If modifying guard chain
- [ ] [Relational Contexts](103_relational_contexts.md) - If affecting context isolation

## Deployment Checklist

Before deploying privacy-sensitive code:

- [ ] **Privacy Audit**: Code reviewed by privacy-focused reviewer
- [ ] **Leakage Analysis**: All metadata exposure documented and budgeted
- [ ] **Side Channel Review**: Timing/size/error channels analyzed
- [ ] **Context Isolation Verified**: Cross-context isolation tested
- [ ] **Guard Chain Tested**: All sends go through complete guard chain
- [ ] **Observability Review**: Logs/metrics don't leak sensitive information

## Common Patterns

### Pattern: Privacy-Aware Error Handling

```rust
// ❌ Bad: Reveals specific capability
return Err(AuraError::authorization_failed(
    "Missing capability: admin_access"
));

// ✅ Good: Generic error
return Err(AuraError::authorization_failed(
    "Insufficient authorization"
));
```

### Pattern: Context-Scoped Messaging

```rust
// ❌ Bad: No context scoping
let envelope = Envelope::new(payload);

// ✅ Good: Explicit relationship scoping
let envelope = Envelope::new_scoped(
    payload,
    relationship_id,
    None, // Blinded capability hint
);
```

### Pattern: Privacy-Preserving Selection

```rust
// ❌ Bad: Exposes detailed requirements
let criteria = SelectionCriteria {
    required_capabilities: vec!["admin", "threshold_signing"],
    min_reputation: 0.9,
};

// ✅ Good: Blinded criteria
let criteria = PrivacyAwareSelectionCriteria::for_relationship(rel_id)
    .require_capability("threshold_signing") // Will be blinded
    .min_reliability(ReliabilityLevel::High)
    .prefer_privacy_features(true);
```

### Pattern: No Side-Channel Failures

```rust
// ❌ Bad: Different failure paths leak information
if !has_capability {
    return Err(quick_error()); // Fast path
}
if over_budget {
    check_budget().await?; // Slow path - timing side channel!
}

// ✅ Good: Uniform failure handling
let guard_result = evaluate_all_guards(&snapshot); // Uniform evaluation
if guard_result.is_err() {
    return Err(generic_error()); // Same code path
}
```

## References

- [Information Flow Contract](003_information_flow_contract.md)
- [Transport and Information Flow](108_transport_and_information_flow.md)
- [Authorization](109_authorization.md)
- [Relational Contexts](103_relational_contexts.md)
- `crates/aura-transport/` - Reference implementation
- `crates/aura-protocol/src/guards/` - Guard chain implementation
