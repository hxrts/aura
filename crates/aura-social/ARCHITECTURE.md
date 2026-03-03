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
## Boundaries
- Chat message handling lives in aura-chat.
- Transport coordination lives in aura-protocol.
- Runtime social state lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.

