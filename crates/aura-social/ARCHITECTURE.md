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
- Membership and stewardship changes should follow approved workflows.
- Home relationships define trust boundaries.

## Boundaries
- Chat message handling lives in aura-chat.
- Transport coordination lives in aura-protocol.
- Runtime social state lives in aura-agent.

## Operation Categories
See `OPERATION_CATEGORIES` in `src/lib.rs` for the current A/B/C table.
