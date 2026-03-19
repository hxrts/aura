# Aura Social (Layer 5) - Architecture and Invariants

## Purpose
Social topology and moderation layer providing home management, neighborhood
discovery, relay selection, and content moderation for the social graph.

## Inputs
- aura-core (effect traits, identifiers).
- aura-journal (fact infrastructure, social facts from journal).

## Outputs
- `SocialFact`, `SocialFactReducer` for social state facts.
- `Home`, `Neighborhood` for social graph structure.
- `SocialTopology`, `DiscoveryLayer` for graph traversal.
- `TraversalService` for path finding.
- `ModerationPolicy`, `ModerationAction` for content moderation.
- `RelayCandidateBuilder`, `ReachabilityChecker` for relay selection.
- `HomeAvailability`, `NeighborhoodAvailability` for availability tracking.
- `StorageService` for social data persistence.

## Invariants
- Facts must be reduced under their matching `ContextId`.
- Membership and moderatorship changes should follow approved workflows.
- Home relationships define trust boundaries.

## Ownership Model

- `aura-social` is primarily `Pure` social-topology and view/reducer logic.
- Any exclusive moderation or topology transition semantics should remain
  explicit and `MoveOwned`.
- Long-lived topology/discovery ownership belongs in explicit `ActorOwned`
  higher-layer coordinators rather than hidden mutable crate state.
- Capability-gated publication is required for parity-critical social facts.
- `Observed` social views are downstream and must not author semantic truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/topology/view logic | `Pure` | Deterministic social fact reduction and topology derivation. |
| moderation and topology-transition semantics | `MoveOwned` | Exclusive moderation or trust-boundary transitions remain explicit. |
| long-lived topology/discovery ownership | none local | Runtime discovery/topology coordination belongs in higher-layer services. |
| capability-gated publication | typed social workflow boundary | Social fact publication remains explicit and auditable. |
| Observed-only surfaces | topology/view consumers only | UI/runtime views stay downstream of authoritative social state. |

### Capability-Gated Points

- parity-critical social fact publication
- moderation and membership transition flows consumed by higher-layer runtime
  services

### Verification Hooks

- `cargo check -p aura-social`
- `cargo test -p aura-social -- --nocapture`

### Detailed Specifications

### InvariantSocialBoundaryScopedMembership
Social topology membership and moderatorship updates remain scoped to explicit trust boundaries.

Enforcement locus:
- src social fact reducers validate boundary and membership transitions.
- Topology updates require evidence through journal-backed facts.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-social

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines boundary isolation semantics.
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) defines relationship and neighborhood boundaries.
## Testing

### Strategy

Boundary-scoped membership and access level computation are the primary
concerns. Integration tests in `tests/topology/` verify access levels,
role enforcement, and simulation scenarios. General integration tests stay
top-level. Inline tests verify individual components (home, neighborhood,
topology, access, storage).

### Running tests

```
cargo test -p aura-social
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Access level computed wrong for hop distance | `src/access.rs` (14 inline), `tests/topology/role_access_e2e.rs` | Covered |
| Invalid access override accepted | `src/access.rs` `test_determine_access_level_ignores_invalid_override_transition` | Covered |
| Relationship downgrade violates trust boundary | `src/topology.rs` `test_relationship_priority` | Covered |
| Neighborhood construction non-deterministic | `src/neighborhood.rs` `test_from_facts_is_deterministic` | Covered |
| Duplicate membership inflates member count | `src/neighborhood.rs` `test_invariant_membership_unique` | Covered |
| Home at capacity accepts new member | `src/home.rs` `test_validate_join`, `test_home_capacity` | Covered |
| Discovery layer priority wrong | `tests/integration_tests.rs` (28 tests) | Covered |
| Relay candidate selection wrong | `src/relay/candidates.rs` (4 inline), `tests/integration_tests.rs` | Covered |
| Storage allocation exceeds budget | `src/storage.rs` (6 inline) | Covered |
| Partition causes topology divergence | `tests/topology/simulation_tests.rs` (23 tests) | Covered |
| Access level properties violated | `tests/topology/property_access_levels.rs` (4 proptests) | Covered |

## Boundaries
- Chat message handling lives in aura-chat.
- Transport coordination lives in aura-protocol.
- Runtime social state lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
