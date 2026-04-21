# Aura Social (Layer 5)

## Purpose

Neighborhood-plane topology and moderation layer providing home management, neighborhood discovery, permit classification, and neighborhood-derived candidate production.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Social facts, reducers, and topology derivation | Chat message handling (aura-chat) |
| Home and neighborhood management | Transport coordination (aura-protocol) |
| Moderation policy and actions | Runtime social state (aura-agent) |
| Neighborhood-derived candidate production and reachability | Final route or retrieval selection |
| Access level computation and role enforcement | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Incoming | aura-core | Effect traits, identifiers |
| Incoming | aura-journal | Fact infrastructure, social facts from journal |
| Incoming | aura-macros | Domain fact derive macros |
| Outgoing | — | `SocialFact`, `SocialFactReducer` for social state facts |
| Outgoing | — | `Home`, `Neighborhood` for social graph structure |
| Outgoing | — | `SocialTopology`, `DiscoveryLayer` for graph traversal |
| Outgoing | — | `TraversalService` for path finding |
| Outgoing | — | `ModerationPolicy`, `ModerationAction` for content moderation |
| Outgoing | — | `RelayCandidateBuilder`, `ReachabilityChecker` for neighborhood candidate production |
| Outgoing | — | `HomeAvailability`, `NeighborhoodAvailability` for availability tracking |
| Outgoing | — | `StorageService` for social data persistence |

## Invariants

- Facts must be reduced under their matching `ContextId`.
- Membership and moderatorship changes should follow approved workflows.
- Home relationships define trust boundaries.

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

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-social` is primarily `Pure` social-topology and view/reducer logic.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| facts/reducers/topology/view logic | `Pure` | Deterministic social fact reduction and topology derivation. |
| moderation and topology-transition semantics | `MoveOwned` | Exclusive moderation or trust-boundary transitions remain explicit. |
| long-lived topology/discovery ownership | none local | Runtime discovery/topology coordination belongs in higher-layer services. |
| capability-gated publication | typed social workflow boundary | Social fact publication remains explicit and auditable. |
| Observed-only surfaces | `Observed` | UI/runtime views stay downstream of authoritative social state. |

### Capability-Gated Points

- parity-critical social fact publication
- moderation and membership transition flows consumed by higher-layer runtime services

## Testing

### Strategy

Boundary-scoped membership and access level computation are the primary concerns. Integration tests in `tests/topology/` verify access levels, role enforcement, and simulation scenarios. General integration tests stay top-level. Inline tests verify individual components (home, neighborhood, topology, access, storage).

### Commands

```
cargo test -p aura-social
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Access level computed wrong for hop distance | `src/access.rs` (14 inline), `tests/topology/role_access_e2e.rs` | Covered |
| Invalid access override accepted | `src/access.rs` `test_determine_access_level_ignores_invalid_override_transition` | Covered |
| Relationship downgrade violates trust boundary | `src/topology.rs` `test_relationship_priority` | Covered |
| Moderator who isn't member gets capability | `src/home.rs` `test_moderator_designation_requires_member_membership` | Covered |
| Neighborhood construction non-deterministic | `src/neighborhood.rs` `test_from_facts_is_deterministic` | Covered |
| Duplicate membership inflates member count | `src/neighborhood.rs` `test_invariant_membership_unique` | Covered |
| Home at capacity accepts new member | `src/home.rs` `test_validate_join`, `test_home_capacity` | Covered |
| Access computation non-deterministic | `tests/topology/role_access_e2e.rs` `test_access_computation_is_deterministic_for_identical_fact_sets` | Covered |
| Relay candidate selection wrong | `src/relay/candidates.rs` (4 inline), `tests/integration_tests.rs` | Covered |
| Storage allocation exceeds budget | `src/storage.rs` (6 inline) | Covered |
| Partition causes topology divergence | `tests/topology/simulation_tests.rs` (23 tests) | Covered |
| Access level properties violated | `tests/topology/property_access_levels.rs` (4 proptests) | Covered |

## Operation Categories

See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

## References

- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md)
- [Social Architecture](../../docs/115_social_architecture.md)
- [Operation Categories](../../docs/109_operation_categories.md)
