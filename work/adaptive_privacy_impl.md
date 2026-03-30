# Adaptive Privacy Routing and Unified Service Surfaces — Implementation Plan

## Context

This plan aligns implementation work with [adaptive_privacy_3.md](/Users/hxrts/projects/aura/work/adaptive_privacy_3.md).

The revised proposal now has one central architectural objective:

- converge rendezvous, relay, deferred delivery, and distributed caching onto a shared service model built from `Establish`, `Move`, and `Hold`

This work is driven in part by a concrete current problem: Aura's relay model leaks social structure through traffic patterns. The implementation plan therefore has two linked goals:

- reduce or remove that social-graph leakage
- avoid carrying social-architecture vocabulary directly into the new interface layer

The build and configuration model follows from this:

- the family model (`Establish`, `Move`, `Hold`) is always active after migration — it is the refactored architecture, not an optional overlay
- `Hold` is deployable before privacy for availability — neighborhood-scoped temporary retention with selector-based retrieval improves delivery under partition and offline peers, independent of mixing or cover
- once privacy ships, the adaptive controller computes `LocalRoutingProfile` from local conditions under a single fixed policy (constants, minimums, targeting curves) that ships with the build — users have no knobs because network privacy depends on socializing its cost across all participants
- during development and simulation, the policy parameters themselves are tunable so we can sweep, test controller stability, and converge on the fixed policy that goes to production
- envelope crypto is the one compile-time gate: default builds use encrypted envelopes; `transparent_onion` is for debugging and simulation only

Production deployment has three stages: (1) family model with Hold for availability, (2) privacy policy tuning in simulation, (3) full privacy deployment with encrypted envelopes and the tuned fixed policy. Two build modes, not three. Simulation tunes the policy; production deploys a fixed result.

That means the implementation plan must prioritize:

- family and object normalization
- clean ownership of local caches in `aura-agent`
- clear policy-surface boundaries for `Discover`, `Permit`, `Transfer`, and `Select`
- canonical anchor types for shared descriptors, moved envelopes, held objects, and runtime selection state
- a shared `Hold` custody substrate with explicit named profiles for deferred delivery and distributed caching
- abstract service interfaces that can be fulfilled by socially rooted actors today without being defined in terms of those actors
- social provisioning split cleanly between the `Neighborhood Plane` and the `Web of Trust Plane`
- direct-friend trust relationships modeled as relational contexts rather than authority objects
- friends of friends treated as local derivation or bounded introduction evidence, not canonical shared graph state
- neighborhood-budgeted reciprocal accountability for `Hold` without per-deposit attribution

The biggest implementation risk is over-generalizing before the first concrete service boundary stabilizes. This plan therefore prioritizes migration of current rendezvous, relay, and cache ownership seams before broad new feature spread.

Reference documents:

- `work/adaptive_privacy_3.md`
- `work/relay.md`
- `work/style_guide_docs.md`
- `docs/003_information_flow_contract.md`
- `docs/104_runtime.md`
- `docs/111_transport_and_information_flow.md`
- `docs/113_rendezvous.md`
- `docs/114_relational_contexts.md`
- `docs/115_social_architecture.md`
- `docs/116_maintenance.md`
- `docs/122_ownership_model.md`

Key design sections in `work/adaptive_privacy_3.md`:

- [Why This Model](./adaptive_privacy_3.md#25-why-this-model)
- [Service Taxonomy](./adaptive_privacy_3.md#5-service-taxonomy)
- [Object Taxonomy](./adaptive_privacy_3.md#52-object-taxonomy)
- [Policy Surfaces](./adaptive_privacy_3.md#53-policy-surfaces)
- [Matrix](./adaptive_privacy_3.md#54-matrix)
- [Composition Logic](./adaptive_privacy_3.md#55-composition-logic)
- [Worked Examples](./adaptive_privacy_3.md#61-worked-examples)
- [Clean Abstraction Boundaries](./adaptive_privacy_3.md#7-clean-abstraction-boundaries)
- [Migration From Current Aura](./adaptive_privacy_3.md#77-migration-from-current-aura)
- [Key Objects And Interfaces](./adaptive_privacy_3.md#8-key-objects-and-interfaces)
- [Adaptive Runtime Policy](./adaptive_privacy_3.md#9-adaptive-runtime-policy)
- [Accountability and Limits](./adaptive_privacy_3.md#10-accountability-and-limits)
- [Integration With Aura](./adaptive_privacy_3.md#11-integration-with-aura)
- [Key Invariants](./adaptive_privacy_3.md#12-key-invariants)
- [Open Questions and Validation Obligations](./adaptive_privacy_3.md#13-open-questions-and-validation-obligations)

Primary current code seams to use as anchors:

- [crates/aura-rendezvous/src/facts.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/facts.rs)
- [crates/aura-rendezvous/src/service.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/service.rs)
- [crates/aura-rendezvous/src/descriptor.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/descriptor.rs)
- [crates/aura-social/src/topology.rs](/Users/hxrts/projects/aura/crates/aura-social/src/topology.rs)
- [crates/aura-social/src/relay/candidates.rs](/Users/hxrts/projects/aura/crates/aura-social/src/relay/candidates.rs)
- [crates/aura-relational](/Users/hxrts/projects/aura/crates/aura-relational)
- [crates/aura-core/src/effects/relay.rs](/Users/hxrts/projects/aura/crates/aura-core/src/effects/relay.rs)
- [crates/aura-agent/src/runtime/services/rendezvous_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/rendezvous_manager.rs)
- [crates/aura-agent/src/runtime/services/social_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/social_manager.rs)
- [crates/aura-agent/src/handlers/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/handlers/rendezvous.rs)
- [crates/aura-sync/src/infrastructure/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-sync/src/infrastructure/rendezvous.rs)
- [crates/aura-macros/src](/Users/hxrts/projects/aura/crates/aura-macros/src)
- [scripts/check](/Users/hxrts/projects/aura/scripts/check)

## Phase 1 — Normalize Families, Objects, and Policy Surfaces

Goal: define the shared family vocabulary, object taxonomy, and policy-surface checklist before changing runtime behavior.

Reference:
- [Service Taxonomy](./adaptive_privacy_3.md#5-service-taxonomy)
- [Social Provisioning Comes From Two Planes](./adaptive_privacy_3.md#social-provisioning-comes-from-two-planes)
- [Object Taxonomy](./adaptive_privacy_3.md#52-object-taxonomy)
- [Policy Surfaces](./adaptive_privacy_3.md#53-policy-surfaces)
- [Matrix](./adaptive_privacy_3.md#54-matrix)
- [Social Architecture](./adaptive_privacy_3.md#72-social-architecture)
- [Trust Evidence Lifecycle](./adaptive_privacy_3.md#trust-evidence-lifecycle)
- [Plane Ownership In Implementation](./adaptive_privacy_3.md#plane-ownership-in-implementation)
- [Migration From Current Aura](./adaptive_privacy_3.md#77-migration-from-current-aura)

This phase is also where we prevent the current leak shape from reappearing in new names. The interfaces must not assume that the provider is a home peer, neighborhood actor, direct friend, introduced FoF, guardian, or other social role, even if those are the first concrete providers we expect to use.

Likely edit targets:

- [crates/aura-rendezvous/src/facts.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/facts.rs)
- [crates/aura-rendezvous/src/descriptor.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/descriptor.rs)
- [crates/aura-core/src/effects/relay.rs](/Users/hxrts/projects/aura/crates/aura-core/src/effects/relay.rs)
- [crates/aura-social/src/topology.rs](/Users/hxrts/projects/aura/crates/aura-social/src/topology.rs)
- [crates/aura-social/src/relay/candidates.rs](/Users/hxrts/projects/aura/crates/aura-social/src/relay/candidates.rs)
- [crates/aura-relational](/Users/hxrts/projects/aura/crates/aura-relational)
- [docs/113_rendezvous.md](/Users/hxrts/projects/aura/docs/113_rendezvous.md)
- [docs/114_relational_contexts.md](/Users/hxrts/projects/aura/docs/114_relational_contexts.md)
- [docs/115_social_architecture.md](/Users/hxrts/projects/aura/docs/115_social_architecture.md)
- [docs/116_maintenance.md](/Users/hxrts/projects/aura/docs/116_maintenance.md)

How to carry this out:

1. Start in [crates/aura-rendezvous/src/facts.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/facts.rs) and split today's overloaded descriptor/hint model into:
   - abstract service-surface advertisement
   - concrete connectivity endpoints
2. Update [crates/aura-rendezvous/src/descriptor.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/descriptor.rs) to consume the new descriptor shape without reintroducing route policy.
3. Update [crates/aura-core/src/effects/relay.rs](/Users/hxrts/projects/aura/crates/aura-core/src/effects/relay.rs) only for the pure shared family/object vocabulary; do not move runtime policy into it.
4. Keep [crates/aura-social/src/topology.rs](/Users/hxrts/projects/aura/crates/aura-social/src/topology.rs) and [crates/aura-social/src/relay/candidates.rs](/Users/hxrts/projects/aura/crates/aura-social/src/relay/candidates.rs) authoritative for neighborhood facts and locality-derived candidates only.
5. Use [crates/aura-relational](/Users/hxrts/projects/aura/crates/aura-relational) for direct-friend relational contexts and bounded introduction/endorsement facts rather than inventing new authority-shaped trust objects.
6. Make [crates/aura-agent/src/runtime/services/social_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/social_manager.rs) compose those plane-specific outputs into runtime-local provider candidates and final selection inputs.
7. Update the docs in the same phase so terminology and boundaries land together.

### 1.1 Introduce family and object vocabulary

- [x] Add shared family vocabulary such as:
      `Establish`
      `Move`
      `Hold`
- [x] Add or normalize object types for:
      authoritative shared objects
      transport/protocol objects
      runtime-derived local objects
      proof/accounting objects
- [x] Define canonical anchor types for the first implementation pass:
      `ServiceDescriptor`
      `MoveEnvelope`
      `HeldObject`
      `RetrievalCapability`
      `ProviderCandidate`
      `SelectionState`
- [x] Add unit tests for object-type invariants and serialization boundaries

**Success**: Aura has one family and object vocabulary for service surfaces, with runtime-derived local objects clearly separated from authoritative shared objects.

### 1.2 Split connectivity from service advertisement

- [x] Add or normalize:
      `LinkProtocol`
      `LinkEndpoint`
      `RelayHop`
      `Route`
- [x] Split existing `TransportHint` semantics into:
      direct connectivity endpoints
      provider or service-surface advertisements
- [x] Ensure path-establishment metadata stays separate from semantic payload context
- [x] Add tests for direct, multi-hop, and zero-hop route representations
- [x] Remove or deprecate legacy `TransportHint` cases and helpers that mix direct connectivity, relay fallback, and route policy in one type
- [x] Delete call sites that still treat rendezvous hints as final routing decisions rather than advisory service-surface input
- [x] Keep any compatibility shim short-lived and schedule its deletion in this phase or the next immediate phase

**Success**: connectivity, provider advertisement, and route choice are no longer conflated.

### 1.3 Introduce service-surface descriptors

- [x] Define shared descriptor headers for:
      provider authority and optional device
      service scope
      validity window and epoch binding
      capability family
      common limits and optional non-authoritative quality hints
- [x] Define service-surface descriptors for `Establish`, `Move`, and `Hold`
- [x] Ensure deferred-delivery and distributed-cache descriptors are modeled as different named profiles over the same `Hold` custody substrate
- [x] Make `Discover`, `Permit`, `Transfer`, and `Select` answer strict questions in code and docs:
      discovery data
      admission/policy
      execution/accounting
      runtime ownership
- [x] Ensure descriptor and interface vocabulary is abstract enough to be implemented by social actors today without hardcoding social-role terms into the interface types

**Success**: the system can describe multiple services without inventing a second discovery stack.

### 1.4 Refactor social topology toward Neighborhood/WoT permit-only candidate production

- [x] Keep `aura-social` authoritative for neighborhood facts, membership, locality-scoped classification, and neighborhood-derived candidate production only
- [x] Keep `aura-relational` authoritative for direct-friend relational contexts and bounded introduction/endorsement evidence only
- [x] Refactor `SocialTopology`, relational-context adapters, and related types so they do not own final route or retrieval choice
- [x] Keep `Contact` as unilateral reachability/identification state distinct from bilateral direct-friend trust
- [x] Model direct-friend trust as bilateral relational contexts between authorities rather than new authority objects
- [x] Model FoF as local derivation or bounded introduction/endorsement evidence rather than canonical shared graph state
- [x] Represent shared trust inputs as evidence provenance rather than canonical shared policy tiers; derive any coarse selection tier locally in the runtime
- [x] Extend candidate production to support trust evidence such as `direct_friend`, `introduced_fof`, `neighborhood`, `guardian`, and descriptor fallback where needed, but keep selection out of `aura-social`
- [x] Define explicit expiry, revocation, depth, and fan-out limits for introduction/endorsement artifacts
- [x] Add tests proving social outputs are candidate and permit inputs, not route commitments or observable service-class changes
- [x] Add tests proving the same `Establish`, `Move`, and `Hold` interfaces work under neighborhood-only and WoT-assisted provider selection
- [x] Remove legacy route-selection assumptions from `aura-social` once the runtime selection owner is in place
- [x] Delete helper paths that preserve final-route ownership in `aura-social` beyond the migration window

**Success**: neighborhood facts and trust evidence remain plane-owned inputs while `aura-agent` owns the fused runtime `Permit` view and final selection.

### 1.5 Separate rendezvous semantics from runtime cache ownership

- [x] Keep `aura-rendezvous` focused on path/object semantics, publication, validation, and establish bootstrap
- [x] Remove or de-emphasize mutable runtime-owned descriptor cache responsibility from Layer 5 surfaces
- [x] Move canonical mutable descriptor caching responsibility to `aura-agent`
- [x] Add tests ensuring rendezvous can prepare and validate service advertisements without owning the long-lived cache
- [x] Remove legacy mutable descriptor-cache ownership from Layer 5 code paths once the runtime registry is canonical
- [x] Delete duplicate cache accessors that allow callers to bypass the runtime-owned descriptor cache

**Success**: rendezvous advertises and validates path objects but does not own the runtime registry.

### 1.6 Update authoritative documentation

- [x] Update `docs/113_rendezvous.md` to describe service-surface advertisement rather than overloaded transport hints
- [x] Update `docs/114_relational_contexts.md` to describe direct-friend trust as relational-context state rather than authority identity
- [x] Update `docs/115_social_architecture.md` to distinguish the `Neighborhood Plane` from the `Web of Trust Plane` and emphasize permit classification and candidate production rather than route ownership
- [x] Update docs to distinguish trust evidence provenance from runtime-local policy tiers so shared types do not harden WoT-specific enums into wire-visible APIs
- [x] Update `docs/116_maintenance.md` and related maintenance docs to clarify that service caches remain local, actor-owned, and fact-invalidated
- [x] Update affected crate `ARCHITECTURE.md` files for `aura-rendezvous`, `aura-social`, `aura-relational`, `aura-agent`, and `aura-sync`
- [x] Make all documentation updates in this phase adhere to [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)

**Success**: the docs reflect the new family, object, and policy-surface split.

### 1.7 Define the first canonical service definition template

- [x] Add a required design template for each concrete service covering:
      families used
      authoritative shared objects, protocol objects, and runtime-local state
      discovery descriptors or facts
      permit policy
      effect interfaces and handlers
      runtime owner and caches
- [x] Apply the template to at least one concrete current service during the migration

Required template for concrete services:

- `Service name`: short stable identifier
- `Families used`: which of `Establish`, `Move`, `Hold` the service exercises
- `Authoritative shared objects`: journal facts and other shared objects that define truth
- `Protocol objects`: envelopes, proofs, receipts, or protocol-state objects exchanged in motion
- `Runtime-local state`: selection state, caches, health views, retry budgets, and other local derivations
- `Discovery surface`: descriptors, facts, or indexes used to discover candidate providers
- `Permit inputs`: neighborhood facts, trust evidence provenance, capabilities, budgets, and local policy inputs
- `Transfer path`: handler and effect surfaces that execute the service
- `Runtime owner`: the crate, service, and actor boundary that owns long-lived mutation
- `Cache policy`: what cache exists, who owns it, and which facts invalidate it
- `Privacy notes`: what must remain non-wire-visible and which local derivations must not become shared enums

Applied template: `Rendezvous Establish`

- `Service name`: `rendezvous_establish`
- `Families used`: `Establish`
- `Authoritative shared objects`: `RendezvousFact::Descriptor`, `RendezvousFact::ChannelEstablished`
- `Protocol objects`: `NoiseHandshake`, handshake receipts, selected connectivity endpoint
- `Runtime-local state`: descriptor snapshot, selected transport, retry budget, channel warmup state
- `Discovery surface`: runtime-owned `RendezvousDescriptor` snapshot, derived `LinkEndpoint`, derived `ServiceDescriptor`
- `Permit inputs`: context capabilities, flow budget, neighborhood-plane candidates, web-of-trust evidence provenance, local health
- `Transfer path`: `RendezvousService::prepare_establish_channel()` plus runtime transport and journal handlers
- `Runtime owner`: `aura-agent::runtime::services::RendezvousManager`
- `Cache policy`: descriptor cache is actor-owned in `aura-agent`; descriptor facts and invalidation facts refresh or evict entries; `aura-rendezvous` does not own the long-lived cache
- `Privacy notes`: provider choice stays runtime-local; shared descriptor facts do not reveal whether neighborhood or web-of-trust evidence dominated selection

**Success**: future services have a consistent design contract instead of ad hoc per-subsystem shape.

### 1.8 Add forward-looking architectural enforcement

- [x] Add proc-macro declaration surfaces, or extend existing ones, so concrete services can declare:
      families used
      object categories used
      discover/permit/transfer/select ownership points
      authoritative versus runtime-local boundaries
- [x] Add Rust-native lint coverage, compile-fail coverage, or both for the most important shape rules:
      service-surface types must not hardcode social-role terms
      runtime-local caches must not appear as authoritative shared objects
      concrete services must declare ownership and policy surfaces in sanctioned locations
- [x] Add thin script checks only where type or lint enforcement cannot realistically prove the rule, for example:
      docs and `ARCHITECTURE.md` declarations stay synchronized
      required macro annotations exist on designated service boundary modules
- [x] Define the default CI lane for these checks so new work is gated automatically rather than by convention
- [x] Require any temporary migration exception, allowlist, or compatibility alias to carry an owner and explicit removal condition

**Success**: policy adherence is enforced by types, macros, lints, compile-fail tests, and only minimally by scripts.

Likely edit targets:

- [crates/aura-macros/src](/Users/hxrts/projects/aura/crates/aura-macros/src)
- new `trybuild` tests under the owning crates, likely `crates/aura-agent/tests/` and `crates/aura-rendezvous/tests/`
- [scripts/check](/Users/hxrts/projects/aura/scripts/check), especially wrappers similar in spirit to existing ownership and architecture gates

How to carry this out:

1. Prefer extending [crates/aura-macros/src](/Users/hxrts/projects/aura/crates/aura-macros/src) with service-boundary declarations rather than inventing new free-form comments.
2. Add compile-fail coverage for the most important boundaries before adding shell checks.
3. Add a thin wrapper in [scripts/check](/Users/hxrts/projects/aura/scripts/check) only when the rule cannot be expressed cleanly in macros, visibility, or lints.

### 1.9 Inventory and remove legacy vocabulary and object usage

- [x] Add a documented `rg` sweep list for legacy terms, legacy object names, and overloaded interface names that must disappear during the migration
- [x] Run an initial repo-wide search for legacy terms such as:
      `TransportHint`
      mailbox and mailbox-polling terms
      relay-specific route-policy names that should become `Establish`/`Move`
      legacy retained-object and cache helper names that should become `Hold`
      social-role-shaped provider or schema field names that leak into interface types
- [x] Record the initial search results in the implementation notes or adjacent migration docs so the remaining refactor surface is explicit
- [x] Convert every remaining matched production call site, object type, helper, and schema field to the new family/object vocabulary or remove it entirely
- [x] Keep transitional aliases or compatibility shims short-lived, explicitly marked, and included in the same search inventory until deleted
- [x] Add follow-up `rg` sweeps at the end of each migration phase that touches renamed objects or legacy terminology
- [x] Prefer keeping the canonical sweep list in-tree, for example in a small script under [scripts/check](/Users/hxrts/projects/aura/scripts/check) or an adjacent migration helper, so contributors can rerun the same inventory consistently

Phase 1 inventory artifacts:

- canonical sweep script: [scripts/check/adaptive-privacy-legacy-sweep.sh](/Users/hxrts/projects/aura/scripts/check/adaptive-privacy-legacy-sweep.sh)
- tracked inventory and removal owners: [adaptive_privacy_legacy_inventory.md](/Users/hxrts/projects/aura/work/adaptive_privacy_legacy_inventory.md)

**Success**: legacy object names and legacy terms are tracked explicitly and driven to zero rather than disappearing by convention.

### 1.10 Phase 1 legacy cleanup

- [x] Re-run the documented legacy-term sweep and update the tracked inventory with what Phase 1 actually removed
- [x] Delete any `TransportHint` compatibility helpers, aliases, or bridge code that are no longer required after the family/object split lands
- [x] Mark every surviving legacy term, object, or shim with an explicit owner and the earliest phase where it must be removed
- [x] Fail the phase if newly introduced family/object terminology coexists with equivalent legacy names in production paths without an explicit migration note

### Phase 1 verification and closeout

- [x] `cargo check -p aura-core -p aura-social -p aura-rendezvous -p aura-sync -p aura-agent`
- [x] `cargo test -p aura-core -p aura-social -p aura-rendezvous -p aura-sync -p aura-agent`
- [x] `just check-arch`
- [x] `just lint-arch-syntax`
- [x] `just ci-ownership-policy`
- [x] Stage non-gitignored files touched in this phase
- [x] Commit with a focused message, for example:
      `git commit -m "adaptive-privacy: normalize families and service objects"`

## Phase 2 — Build the Unified Local Service Registry

Goal: create the canonical runtime-owned registry and cache layer in `aura-agent`.

Reference:
- [Policy Surface Mapping To Aura](./adaptive_privacy_3.md#6-policy-surface-mapping-to-aura)
- [Clean Abstraction Boundaries](./adaptive_privacy_3.md#7-clean-abstraction-boundaries)
- [Web Of Trust Plane](./adaptive_privacy_3.md#web-of-trust-plane)
- [Trust Evidence Lifecycle](./adaptive_privacy_3.md#trust-evidence-lifecycle)
- [Plane Ownership In Implementation](./adaptive_privacy_3.md#plane-ownership-in-implementation)
- [Migration From Current Aura](./adaptive_privacy_3.md#77-migration-from-current-aura)
- [Integration With Aura](./adaptive_privacy_3.md#11-integration-with-aura)

Likely edit targets:

- [crates/aura-agent/src/runtime/services/rendezvous_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/rendezvous_manager.rs)
- [crates/aura-agent/src/runtime/services/social_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/social_manager.rs)
- [crates/aura-agent/src/handlers/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/handlers/rendezvous.rs)
- [crates/aura-rendezvous/src/service.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/service.rs)
- [crates/aura-sync/src/infrastructure/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-sync/src/infrastructure/rendezvous.rs)

How to carry this out:

1. Make [crates/aura-agent/src/runtime/services/rendezvous_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/rendezvous_manager.rs) the canonical mutable descriptor-cache owner.
2. Reduce [crates/aura-rendezvous/src/service.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/service.rs) toward pure service semantics and transient coordination rather than long-lived cache ownership.
3. Route ingestion and lookup from [crates/aura-agent/src/handlers/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/handlers/rendezvous.rs) and [crates/aura-sync/src/infrastructure/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-sync/src/infrastructure/rendezvous.rs) through the runtime registry rather than parallel local caches.
4. Keep [crates/aura-agent/src/runtime/services/social_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/social_manager.rs) focused on permit-scored candidates, not authoritative descriptor ownership.

### 2.1 Add a runtime-owned service registry

- [x] Add an `ActorOwned` registry service in `aura-agent`
- [x] Make it the canonical owner of:
      descriptor cache
      provider-health cache
      selector and route state
      local quality or hold observations
- [x] Ensure consumers access it through sanctioned ingress and observed projections only

**Success**: there is one runtime owner for mutable local service views.

### 2.2 Make caches fact-invalidated and epoch-aware

- [x] Key service caches so maintenance invalidation and epoch rotation can invalidate them cleanly
- [x] Reuse the existing maintenance and cache-invalidation model instead of inventing replicated runtime cache truth
- [x] Add tests for invalidation on epoch transition and descriptor expiry

**Success**: service caches behave like local derived views, not distributed truth.

### 2.3 Unify rendezvous-manager and related local descriptor paths

- [x] Route descriptor ingestion through the unified registry
- [x] Remove duplicate or conflicting ownership between rendezvous service-local caches and runtime manager caches
- [x] Add tests proving a single canonical cache path exists at runtime
- [x] Delete legacy descriptor-cache reads and writes that bypass the unified runtime registry
- [x] Remove fallback lookups that silently consult multiple caches in unspecified order

**Success**: the runtime no longer has parallel descriptor cache ownership.

This phase should land before broad protocol expansion. If the canonical registry boundary is still unstable, later work on `Move` and `Hold` will likely duplicate ownership again.

### 2.4 Enforce registry ownership boundaries

- [x] Add lint or compile-fail coverage proving that canonical mutable descriptor and provider caches remain owned by `aura-agent`
- [x] Add proc-macro or typed-boundary annotations for the registry service and its sanctioned ingress/query surfaces
- [x] Add a thin governance script or CI wrapper only if needed to ensure there is no second runtime-owned registry introduced elsewhere
- [x] Add explicit cleanup tasks for any transitional duplicate registry state or bridge adapters introduced during migration

**Success**: duplicate registry ownership becomes hard to express and easy to catch in CI.

### 2.5 Add friend management to the Contacts screen in TUI and Web

The contacts surface should become the user-facing owner for managing the transition from unilateral `Contact` to bilateral `Friend`.

Likely edit targets:

- [crates/aura-app/src/views/contacts.rs](/Users/hxrts/projects/aura/crates/aura-app/src/views/contacts.rs)
- [crates/aura-app/src/views/state.rs](/Users/hxrts/projects/aura/crates/aura-app/src/views/state.rs)
- [crates/aura-app/src/ui_contract.rs](/Users/hxrts/projects/aura/crates/aura-app/src/ui_contract.rs)
- [crates/aura-ui/src/model/mod.rs](/Users/hxrts/projects/aura/crates/aura-ui/src/model/mod.rs)
- [crates/aura-ui/src/keyboard/contacts.rs](/Users/hxrts/projects/aura/crates/aura-ui/src/keyboard/contacts.rs)
- [crates/aura-terminal/src/tui/state/views/contacts.rs](/Users/hxrts/projects/aura/crates/aura-terminal/src/tui/state/views/contacts.rs)
- [crates/aura-terminal/src/tui/state/handlers/screen.rs](/Users/hxrts/projects/aura/crates/aura-terminal/src/tui/state/handlers/screen.rs)
- [crates/aura-web/src](/Users/hxrts/projects/aura/crates/aura-web/src)

- [x] Extend shared contacts view state so each contact can expose the minimal states:
      contact
      pending_outbound
      pending_inbound
      friend
- [x] Add shared actions for:
      send friend request
      accept friend request
      decline friend request
      revoke or remove friendship
- [x] Keep shared UI state driven by runtime-derived view projections from `aura-agent`; the TUI and webapp must not each invent separate friendship state machines
- [x] Add parity-critical UI contract ids and action contracts for friend management on the Contacts screen
- [x] Add TUI contacts-screen actions, focus behavior, details-panel affordances, and modal flows for friend management
- [x] Add browser/web contacts-screen affordances for the same friend-management actions using the shared app facade and sanctioned submission paths
- [x] Ensure both shells present the same semantics:
      contact is unilateral reachability
      friend is bilateral accepted trust
      pending inbound and outbound requests are explicit
      revocation is explicit and not silently downgraded to plain contact
- [x] Add tests proving TUI and web observe the same contacts/friendship projection and expose the same action set on equivalent states
- [x] Add tests proving friend-management UI state comes from relational-context facts and runtime projections rather than shell-local heuristics

**Success**: the Contacts screen becomes the parity-consistent management surface for friend relationships in both the TUI and webapp.

### 2.6 Phase 2 legacy cleanup

- [x] Re-run the legacy sweep for duplicate cache ownership, direct cache bypasses, and shell-local friendship state
- [x] Delete transitional duplicate registry paths, fallback cache lookups, and shell-specific contacts/friendship state machines that Phase 2 makes obsolete
- [x] Remove stale Contacts-screen controls, view fields, and UI contract entries that were replaced by the shared friend-management model
- [x] Record any remaining runtime-registry or contacts-surface legacy paths in the migration inventory with explicit phase-bound removal owners

### Phase 2 verification and closeout

- [x] `cargo check -p aura-agent -p aura-rendezvous -p aura-sync -p aura-app -p aura-ui -p aura-terminal -p aura-web`
- [x] `cargo test -p aura-agent -p aura-rendezvous -p aura-sync -p aura-app -p aura-ui -p aura-terminal -p aura-web`
- [x] `just check-arch`
- [x] `just lint-arch-syntax`
- [x] `just ci-ownership-policy`
- [x] `just ci-user-flow-policy`
- [x] Update any affected crate `ARCHITECTURE.md` files and related docs for the new canonical registry/cache ownership model and contacts-screen friend-management parity, following [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)
- [x] Stage non-gitignored files touched in this phase
- [x] Commit with a focused message, for example:
      `git commit -m "adaptive-privacy: add unified runtime service registry"`

## Phase 3 — Roll Out `Establish` And `Move` On Current Operations

Goal: prove the new family model on existing non-onion operations before adding onion-specific envelope work.

Reference:
- [Composition Logic](./adaptive_privacy_3.md#55-composition-logic)
- [Direct Channel Bootstrap](./adaptive_privacy_3.md#direct-channel-bootstrap)
- [Relay Transport](./adaptive_privacy_3.md#relay-transport)
- [Rendezvous](./adaptive_privacy_3.md#71-rendezvous)
- [Connectivity and Path](./adaptive_privacy_3.md#81-connectivity-and-path)
- [Move Objects](./adaptive_privacy_3.md#82-move-objects)
- [Web Of Trust Plane](./adaptive_privacy_3.md#web-of-trust-plane)
- [Plane Ownership In Implementation](./adaptive_privacy_3.md#plane-ownership-in-implementation)
- [Migration From Current Aura](./adaptive_privacy_3.md#77-migration-from-current-aura)

Likely edit targets:

- [crates/aura-rendezvous/src/facts.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/facts.rs)
- [crates/aura-rendezvous/src/service.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/service.rs)
- [crates/aura-rendezvous/src/descriptor.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/descriptor.rs)
- [crates/aura-core/src/effects/relay.rs](/Users/hxrts/projects/aura/crates/aura-core/src/effects/relay.rs)
- [crates/aura-agent/src/runtime/services/rendezvous_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/rendezvous_manager.rs)
- [crates/aura-agent/src/runtime/services/social_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/social_manager.rs)
- a new runtime move service alongside existing services under [crates/aura-agent/src/runtime/services](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services)
- [crates/aura-sync/src/infrastructure/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-sync/src/infrastructure/rendezvous.rs)

How to carry this out:

1. Make path objects explicit in [crates/aura-rendezvous/src/facts.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/facts.rs), [crates/aura-rendezvous/src/service.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/service.rs), and [crates/aura-rendezvous/src/descriptor.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/descriptor.rs) before introducing any onion-format traffic.
2. Introduce bounded `Move` primitives and a runtime-owned move actor for current direct and relay-like movement first, without `transparent_onion`.
3. Migrate current bootstrap, relay, and object-movement paths onto `Establish` and `Move` so the abstractions are proven on ordinary traffic before privacy hardening depends on them.
4. Keep path and move interfaces abstract: social actors may fulfill them, but home, neighborhood, guardian, and similar roles must not appear in the interface types.

This phase exists specifically to keep onion routing from being the first and only real client of the new families. If `Establish` and `Move` do not simplify current non-onion operations, they are not stable enough for privacy routing yet.

### 3.1 Normalize `Establish` semantics

- [ ] Make path objects explicit in the relevant descriptor and runtime APIs
- [ ] Ensure establish flows consume path objects rather than overloaded transport hints
- [ ] Keep live runtime channels actor-owned even when path objects are first-class
- [ ] Add tests for establish semantics and path-object lifecycle
- [ ] Remove legacy establish flows that still depend on overloaded transport hints or mixed route-policy descriptor fields

**Success**: `Establish` acts on explicit path objects and no longer feels like a conceptual outlier.

### 3.2 Implement bounded `Move` primitives

- [ ] Add enqueue, batch-close, shuffle, flush, replay-window expiry, and bounded eviction primitives
- [ ] Keep them infrastructure-local and free of semantic routing policy
- [ ] Use `RandomEffects` for shuffling so simulation remains deterministic
- [ ] Add tests for bounded memory, deterministic shuffle, duplicate suppression, and replay-window expiration
- [ ] Remove legacy movement helpers that bundle queueing, policy, and social-role assumptions together once the bounded primitives exist

**Success**: move primitives are reusable and bounded before onion routing exists.

### 3.3 Add runtime-owned `Move` actor

- [ ] Add an `ActorOwned` move actor in `aura-agent`
- [ ] Ensure it owns:
      bounded relay buffer state
      replay window state
      flush scheduling
      congestion and backpressure state
- [ ] Ensure UI, harness, and social layers only interact through sanctioned ingress and observed projections
- [ ] Remove legacy ad hoc movement ownership paths once the move actor is canonical

**Success**: movement has a single runtime owner and bounded state on current operations.

### 3.4 Roll current bootstrap and direct-path flows onto `Establish`

- [ ] Route current rendezvous/bootstrap paths through explicit path objects and establish interfaces
- [ ] Update direct upgrade and direct-connect helpers to consume `Establish` contracts instead of legacy descriptor hints
- [ ] Add tests covering zero-hop and direct-upgrade establishment on the new path-object model
- [ ] Remove call sites that bypass `Establish` and still interpret descriptor fields as immediate connect instructions

**Success**: current non-onion path creation already uses the new family boundary.

### 3.5 Roll current relay and transit flows onto `Move`

- [ ] Route current relay-like transport paths through the new move actor and bounded move primitives
- [ ] Keep receipts, budgets, and leakage accounting attached to move operations rather than legacy relay helpers
- [ ] Add tests proving current relay behavior runs through `Move` without requiring transparent onion envelopes
- [ ] Remove legacy relay helper paths that own queueing or next-hop movement outside the move actor

**Success**: today's movement paths already exercise `Move`.

### 3.6 Roll current object movement onto `Move`

- [ ] Route existing cache-seeding, retained-object handoff, or similar object-movement paths through `Move`
- [ ] Keep movement separate from custody so this phase does not absorb `Hold` semantics prematurely
- [ ] Add tests for object handoff through `Move` without mailbox or cache-specific type shapes
- [ ] Remove legacy object-movement helpers that preserve cache- or mailbox-specific transport APIs

**Success**: `Move` is proven on current object movement before it is used as part of onion routing.

### 3.7 Define `LocalRoutingProfile::passthrough()` and prove behavioral equivalence

- [ ] Add a named `passthrough()` preset: mixing depth 0, delay 0, cover rate 0, path diversity 1 (Hold is a service family, always active after migration, not a routing profile parameter)
- [ ] Add simulation or integration tests proving that passthrough produces identical observable routing behavior to the pre-migration system
- [ ] Passthrough is the pre-privacy production default — it ships in the first deployment stage before the adaptive privacy policy is tuned
- [ ] Once privacy ships, passthrough remains available for development and simulation but is replaced in production by the fixed adaptive policy

**Success**: passthrough is proven equivalent to pre-migration routing behavior and serves as the first production deployment policy.

### 3.8 Add leak-oriented regression checks

- [ ] Add tests or architectural checks showing that topology changes affect permit and select behavior without changing interface type shape
- [ ] Add checks that establish and move schemas do not directly hardcode home, neighborhood, or guardian roles
- [ ] Add explicit checks for removal of legacy social-role-specific fields from path and move-surface schemas and related helper types

**Success**: the original leak shape is tested against explicitly, not just assumed to be fixed.

### 3.9 Enforce abstract `Establish` and `Move` schemas

- [ ] Add lint or compile-fail coverage proving that establish-surface and move-surface types do not encode social-role-specific fields directly
- [ ] Add proc-macro declarations for establish and move boundaries so receipt, budget, and ownership points are explicit in code
- [ ] Add CI wiring so establish/move schema checks run by default on future changes
- [ ] Remove transitional adapter types that preserve old social-role-shaped schema fields once callers are migrated

**Success**: social-graph leakage does not re-enter the new family model through schema drift.

### 3.10 Phase 3 legacy cleanup

- [ ] Re-run the legacy sweep for overloaded path hints, relay helpers, direct-connect shortcuts, and mailbox/cache-shaped movement APIs
- [ ] Delete any remaining non-onion movement helpers or schema adapters that bypass `Establish` or `Move`
- [ ] Remove stale tests, fixtures, and helper names that still describe the pre-family transport model
- [ ] Record every surviving pre-family movement path with an explicit owner and removal phase before Phase 4 begins

### Phase 3 verification and closeout

- [ ] `cargo check -p aura-core -p aura-rendezvous -p aura-sync -p aura-agent`
- [ ] `cargo test -p aura-core -p aura-rendezvous -p aura-sync -p aura-agent`
- [ ] `just check-arch`
- [ ] `just lint-arch-syntax`
- [ ] `just ci-ownership-policy`
- [ ] Update docs and any affected crate `ARCHITECTURE.md` files for path objects, establish semantics, and move semantics on current operations, following [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)
- [ ] Stage non-gitignored files touched in this phase
- [ ] Commit with a focused message, for example:
      `git commit -m "adaptive-privacy: roll establish and move onto current operations"`

## Checkpoint — Service Shapes Stable Before Hold

Do not start Phase 4 until all of the following are true:

- [ ] neighborhood-only and WoT-assisted candidate production produce the same `Establish` and `Move` wire/schema shapes
- [ ] trust evidence provenance remains separate from runtime-local selection tiers
- [ ] `aura-social` owns neighborhood facts only, `aura-relational` owns WoT facts only, and `aura-agent` is the only owner that fuses them for selection
- [ ] direct friend, FoF, and contact semantics are documented distinctly enough that implementers cannot collapse them into one relation type

**Success**: `Hold` starts only after service-shape neutrality across social planes is already proven.

## Phase 4 — Add `Hold` Substrate And Named Service Profiles

Goal: define the shared held-object custody substrate used by both deferred delivery and distributed caching after `Establish` and `Move` are already proven on current operations. `Hold` provides immediate availability value (partition tolerance, offline delivery) independent of network privacy, so it is the first feature deployable to production after the family model migration. Selector-based retrieval is used from the start so the retrieval interface is correct before privacy is added later.

Reference:
- [Composition Logic](./adaptive_privacy_3.md#55-composition-logic)
- [Deferred Delivery Through A Socially Rooted Provider](./adaptive_privacy_3.md#deferred-delivery-through-a-socially-rooted-provider)
- [Distributed Cache Seeding](./adaptive_privacy_3.md#distributed-cache-seeding)
- [Neighborhood Plane](./adaptive_privacy_3.md#neighborhood-plane)
- [Deferred Delivery and Retrieval](./adaptive_privacy_3.md#74-deferred-delivery-and-retrieval)
- [Distributed Caching](./adaptive_privacy_3.md#75-distributed-caching)
- [Hold Objects](./adaptive_privacy_3.md#83-hold-objects)
- [Shared Substrate vs Profiles](./adaptive_privacy_3.md#shared-substrate-vs-profiles)
- [Retention and Garbage Collection](./adaptive_privacy_3.md#85-retention-and-garbage-collection)
- [Accountability Traffic As Ambient Mass](./adaptive_privacy_3.md#accountability-traffic-as-ambient-mass)
- [Sync-Blended Retrieval](./adaptive_privacy_3.md#95-sync-blended-retrieval)

Likely edit targets:

- new held-object/domain types in the owning Layer 1 or Layer 2 crates, likely near [crates/aura-rendezvous/src/facts.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/facts.rs) only if they genuinely belong there, otherwise in a new dedicated owning module
- [crates/aura-sync/src/infrastructure/rendezvous.rs](/Users/hxrts/projects/aura/crates/aura-sync/src/infrastructure/rendezvous.rs) for sync-blended retrieval bridges
- [crates/aura-agent/src/runtime/services/rendezvous_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/rendezvous_manager.rs) or a sibling runtime service for local hold-provider interactions

How to carry this out:

1. Do not hide held-object semantics inside invitation or chat code first; start from shared held-object contracts.
2. Keep object custody APIs separate from local cache indexing APIs from the beginning.
3. When in doubt, place shared held-object types in the lowest layer that can own them without pulling runtime state upward.
4. Reuse the pre-onion `Move` rollout where held-object handoff needs movement, rather than inventing a separate transport shape.

The implementation target is one shared custody substrate plus explicit named
profiles:

- `DeferredDeliveryHold`
- `CacheReplicaHold`

This phase should preserve the same abstraction discipline: held-object interfaces must not encode social-role assumptions even when the first providers are socially rooted actors.

It should also keep a hard boundary between held-object custody and local indexing. Custody belongs to `Hold`; indexes, rankings, and cache views remain local runtime state.

Because `Hold` ships to production before privacy, the retrieval interface must be selector-based from day one. If retrieval launches with a simpler identity-addressed or mailbox-style pattern, it will need to be migrated when privacy is added. Using selectors from the start means the interface is correct and only the anonymity properties improve when mixing and cover are enabled later.

Hold providers are scoped to the neighborhood: any peer that is a member of any home in the same neighborhood as the depositor's home is an admissible provider. This provides a large anonymity set under onion routing (the full neighborhood, not just one home) and sufficient capacity for availability. Retention policy is uniform across all neighborhood deposits — no tiering by social distance — because observable differences in treatment would be a side channel that narrows the anonymity set when privacy is enabled. `Hold` remains neighborhood-only in this plan.

Hold GC shares epoch, invalidation, and storage-pressure vocabulary with journal GC ([Distributed Maintenance Architecture](../docs/116_maintenance.md)) but makes decisions locally without threshold agreement. Under passthrough, accountability is bilateral (the holder sees the depositor). Under onion routing, accountability is neighborhood-budgeted and reciprocal: deposits route through indirection hops so the holder cannot identify the specific depositor, and runtime selection/scoring enforces service quality without per-deposit attribution.

Once onion routing is active, this accountability path must use typed
single-use reply blocks rather than direct reverse channels. The implementation
should treat these as SURB-like anonymous reply capabilities for bounded
witness return:

- `MoveReceiptReplyBlock`
- `HoldDepositReplyBlock`
- `HoldRetrievalReplyBlock`
- optional `HoldAuditReplyBlock`

Verification is explicit:

- adjacent peers verify hop-local `Move` witnesses
- retrievers, holders, or bounded auditors verify `Hold` witnesses
- `aura-agent` applies scoring, reciprocal budget, and admission consequences
  only after witness verification succeeds

### 4.1 Introduce held-object contracts

- [ ] Add typed contracts for:
      held-object deposit with requested retention duration
      bounded retention and epoch-scoped expiration
      selector-based retrieval
      retention metadata
      move-to-hold handoff metadata
      capability issuance and validation
- [ ] Add canonical anchor types and shared custody interfaces for:
      `HeldObject`
      `RetrievalCapability`
      `ProviderCandidate`
      `SelectionState`
- [ ] Keep held objects encrypted, content-addressed, and opaque to providers
- [ ] Ensure the hold service does not encode mailbox identity
- [ ] Scope hold providers to the neighborhood (any peer in any home in the same neighborhood)
- [ ] Ensure retention policy is uniform across all neighborhood deposits — no tiered treatment by social distance
- [ ] Keep `Hold` neighborhood-only; do not encode direct-friend or FoF roles into `Hold` interface fields, admission, selection, or retention behavior

**Success**: deferred delivery and distributed cache have a shared custody substrate with explicit anchor types, neighborhood-scoped providers, and uniform retention.

### 4.2 Model deferred delivery as a `Hold` profile

- [ ] Define `DeferredDeliveryHold` as a named profile over the shared `Hold` substrate
- [ ] Keep retrieval selector-based and indirect
- [ ] Implement retrieve-once semantics and re-deposit-on-miss behavior in the profile layer
- [ ] Prohibit direct "poll mailbox" flows in parity-critical paths
- [ ] Add tests proving deferred delivery is not identity-addressed at the network boundary
- [ ] Remove legacy mailbox-shaped terminology and call paths that still model retrieval as named mailbox polling
- [ ] Delete compatibility helpers that preserve identity-addressed retrieval once selector-based retrieval is working

**Success**: mailbox behavior is local interpretation, not network identity.

### 4.3 Model distributed cache as a `Move` + `Hold` profile

- [ ] Define `CacheReplicaHold` as a named profile over the shared `Hold` substrate
- [ ] Define distributed cache seeding and movement as `Move` + `CacheReplicaHold`
- [ ] Keep authoritative truth in journals and facts, not in replicated runtime cache records
- [ ] Implement retrieve-many or retention-window-governed semantics in the profile layer
- [ ] Add tests ensuring cache replicas remain opaque held objects and local indexes remain local
- [ ] Remove legacy designs or helper paths that treat replicated runtime cache views as authoritative shared state

**Success**: distributed caching aligns with the same object model as deferred delivery.

### 4.4 Embed retrieval into sync and anti-entropy

- [ ] Route selector-based retrieval through sync and anti-entropy paths
- [ ] Ensure held-object movement and retrieval can share infrastructure where appropriate
- [ ] Let compatible `Hold` retrieval witnesses and non-urgent accountability replies ride the same sync-blended windows when protocol deadlines allow it
- [ ] Add tests proving retrieval is sync-blended rather than a standalone mailbox polling loop
- [ ] Remove legacy standalone retrieval loops or polling helpers once sync-blended retrieval is working

**Success**: retrieval intent is blended into normal sync behavior.

### 4.5 Implement retrieval-capability rotation

- [ ] Define rotation triggers:
      epoch transition
      receipt or usage state
      protocol-defined validity window
- [ ] Ensure stale capabilities fail closed
- [ ] Add tests for rotation, expiration, and selector refresh

**Success**: retrieval handles remain short-lived and non-semantic.

### 4.6 Add typed reply-block accountability channels

- [ ] Add typed single-use reply-block capabilities for accountability return paths:
      `MoveReceiptReplyBlock`
      `HoldDepositReplyBlock`
      `HoldRetrievalReplyBlock`
      optional `HoldAuditReplyBlock`
- [ ] Ensure reply blocks are command-scoped, single-use, and replay-resistant
- [ ] Keep reply-block handling typed and profile-specific rather than exposing one generic reverse callback surface
- [ ] Route reply-block traffic through the same shared `Move` substrate and envelope family as application and retrieval traffic
- [ ] Add bounded batching, jitter, and scheduling rules so accountability replies do not become immediate one-for-one correlation signals
- [ ] Keep accountability replies out of the first-deployment cover-reduction loop even though they share the same envelope family
- [ ] Add tests proving callback/witness return works under onion routing without exposing direct reverse identity
- [ ] Remove legacy assumptions that accountability callbacks can rely on direct return channels once onion routing is active

**Success**: onion-routed accountability has bounded anonymous reply channels rather than ad hoc callbacks.

### 4.7 Implement bounded rotating holder selection

- [ ] Keep the interface scope neighborhood-wide, but make runtime selection choose a bounded rotating subset of neighborhood holders
- [ ] Define rotation triggers based on health, storage pressure, path diversity, and epoch change
- [ ] Add anti-stickiness constraints so no holder dominates longer than the bounded residency window unless explicit failure handling requires it
- [ ] Ensure rotation does not change the public interface shape or introduce social-role-specific schema fields
- [ ] Add tests proving holder rotation preserves uniform treatment while keeping operational state bounded

**Success**: the anonymity set remains neighborhood-wide while the active operational set is bounded and rotatable.

### 4.8 Implement Hold GC harmonized with journal GC

- [ ] Use the same `Epoch` and `MaintenanceEpoch` primitives for held-object epoch scoping
- [ ] Use the same `CacheInvalidated` fact vocabulary for Hold retention events
- [ ] Use the same storage-pressure signaling shapes for Hold provider capacity
- [ ] Implement epoch-scoped expiration: held objects from prior epochs are GC-eligible on epoch rotation
- [ ] Implement retrieval-capability-driven GC: objects whose capabilities have all expired are GC-eligible
- [ ] Implement storage-pressure eviction: epoch then age priority, uniform across all neighborhood deposits
- [ ] Implement retrieve-once semantics for deferred delivery (GC-eligible after retrieval) and retention-window semantics for cache seeds
- [ ] Add tests proving Hold GC does not require threshold agreement
- [ ] Add tests proving eviction priority does not vary by social distance between depositor and holder

**Success**: Hold GC shares vocabulary with journal GC, makes local decisions, and preserves uniform treatment.

### 4.9 Implement neighborhood-budgeted reciprocal accountability and verifier roles

- [ ] Add local hold-service budget accounting for custody providers
- [ ] Feed retrieval success, refusal, eviction, and miss outcomes into provider preference and reciprocal service weighting
- [ ] Ensure the runtime can lower admission priority and selection weight for chronic under-service without per-deposit attribution
- [ ] Define verifier roles for:
      adjacent-hop `Move` witness verification
      `Hold` deposit witness verification
      `Hold` retrieval witness verification
      optional `Hold` audit verification
- [ ] Ensure local credit, reciprocal budget, and provider scoring only update after witness verification succeeds
- [ ] Add tests proving accountability remains local/runtime-owned rather than turning into depositor-identifying protocol state
- [ ] Document and enforce the boundary that applications needing guaranteed durability must use authoritative replicated state, not `Hold`
- [ ] Add one end-to-end documented flow for `DeferredDeliveryHold` and one for `CacheReplicaHold` so implementation and docs speak about the same profile boundaries

**Success**: `Hold` service quality is governed by a concrete reciprocal budget and scoring loop rather than vague social expectation.

### 4.10 Enforce `Hold` boundary rules

- [ ] Add typed or macro-declared boundary markers for held-object custody APIs versus runtime-local indexing APIs
- [ ] Add compile-fail or lint coverage proving that local cache indexes and rankings are not modeled as held objects
- [ ] Add compile-fail or lint coverage proving witness verification happens before local credit or admission effects are applied
- [ ] Add a thin script check only if needed to keep `ARCHITECTURE.md` declarations and hold-boundary annotations aligned
- [ ] Remove any temporary mixed APIs that expose held-object custody and local indexing through the same surface

**Success**: `Hold` cannot silently expand to absorb local indexing or cache-view ownership.

### 4.11 Phase 4 legacy cleanup

- [ ] Re-run the legacy sweep for mailbox polling, identity-addressed retrieval, cache-as-truth helpers, and non-selector retrieval paths
- [ ] Delete obsolete mailbox terminology, retrieval loops, and compatibility helpers that are superseded by selector-based `Hold`
- [ ] Remove any stale direct-callback accountability assumptions once reply-block flows are canonical
- [ ] Record any remaining pre-`Hold` availability shortcuts with an explicit owner and removal phase before onion-routing work starts

### Phase 4 verification and closeout

- [ ] `cargo check -p aura-sync -p aura-rendezvous -p aura-protocol -p aura-agent`
- [ ] `cargo test -p aura-sync -p aura-rendezvous -p aura-protocol -p aura-agent`
- [ ] `just check-arch`
- [ ] `just lint-arch-syntax`
- [ ] `just ci-ownership-policy`
- [ ] Update docs and any affected crate `ARCHITECTURE.md` files for held-object semantics, selector-based retrieval, `Move` + `Hold` cache seeding, and the Neighborhood/WoT responsibility split, following [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)
- [ ] Stage non-gitignored files touched in this phase
- [ ] Commit with a focused message, for example:
      `git commit -m "adaptive-privacy: add hold semantics for retained objects"`

---

## Checkpoint — Families Proven and Hold Deployable

Do not begin onion-routing work until all of the following are true:

- [ ] `Establish` is the canonical shape for current bootstrap and direct-path setup
- [ ] `Move` is the canonical shape for current relay-like transport and object movement
- [ ] `Hold` is the canonical shape for deferred delivery and distributed cache custody
- [ ] `DeferredDeliveryHold` and `CacheReplicaHold` are explicit named profiles over one shared `Hold` substrate
- [ ] the unified runtime registry in `aura-agent` is the only owner of mutable local descriptor and provider views
- [ ] overloaded `TransportHint` behavior is removed or quarantined behind short-lived migration shims with explicit deletion owners
- [ ] mailbox-shaped polling is removed from parity-critical retrieval paths
- [ ] selector-based retrieval is the only retrieval interface, even before privacy is active
- [ ] Hold providers are neighborhood-scoped with uniform retention policy (no social-distance tiering)
- [ ] direct-friend trust is modeled through relational contexts, not authority-shaped service objects
- [ ] introduced FoF inputs remain local derivation or bounded introduction evidence rather than canonical shared graph state
- [ ] typed single-use reply blocks are the only accountability callback mechanism once onion routing is active
- [ ] runtime selection uses bounded rotating holder subsets inside the wider neighborhood-scoped provider set
- [ ] Hold GC uses shared epoch and invalidation vocabulary with journal GC
- [ ] neighborhood-budgeted reciprocal accountability governs hold-provider scoring and admission priority
- [ ] verifier roles are explicit and local consequences only apply after witness verification
- [ ] Web-of-Trust inputs affect only `Permit` and local weighting, not descriptor shape, route shape, retrieval shape, or retention behavior
- [ ] establish, move, and hold interfaces are free of home/neighborhood/friend/FoF/guardian-style role fields
- [ ] current non-onion operations pass on the new abstractions without requiring `transparent_onion`
- [ ] `LocalRoutingProfile::passthrough()` produces verified behavioral equivalence with pre-migration routing
- [ ] docs and affected crate `ARCHITECTURE.md` files are updated to match the pre-onion design, following [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)

This checkpoint is a hard divider in the migration. If it does not pass cleanly, transparent onion routing should not be added yet.

This checkpoint also marks the point at which the family model with Hold for availability can be deployed to production under the passthrough policy. Hold provides immediate value (partition tolerance, offline delivery) while the privacy policy is tuned in simulation.

---

## Phase 5 — Add Transparent Onion Envelopes And Adaptive Runtime Policy

Goal: add onion-shaped transparent envelopes and adaptive local runtime policy only after the non-onion service model is stable. The adaptive controller computes `LocalRoutingProfile` from local conditions; the only compile-time distinction is envelope crypto (encrypted by default, transparent under `transparent_onion` feature flag for debugging and simulation).

Reference:
- [Relay Transport](./adaptive_privacy_3.md#relay-transport)
- [WoT-Assisted Relay Without A WoT-Shaped Interface](./adaptive_privacy_3.md#wot-assisted-relay-without-a-wot-shaped-interface)
- [Move Objects](./adaptive_privacy_3.md#82-move-objects)
- [Adaptive Runtime Policy](./adaptive_privacy_3.md#9-adaptive-runtime-policy)
- [Accountability Traffic As Ambient Mass](./adaptive_privacy_3.md#accountability-traffic-as-ambient-mass)
- [Sync-Blended Retrieval](./adaptive_privacy_3.md#95-sync-blended-retrieval)
- [Accountability and Limits](./adaptive_privacy_3.md#10-accountability-and-limits)

Likely edit targets:

- [crates/aura-core/src/effects/relay.rs](/Users/hxrts/projects/aura/crates/aura-core/src/effects/relay.rs)
- [crates/aura-rendezvous/src/service.rs](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/service.rs)
- [crates/aura-agent/src/runtime/services/social_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/social_manager.rs)
- [crates/aura-agent/src/runtime/services/rendezvous_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/rendezvous_manager.rs)
- a new runtime move service alongside existing services under [crates/aura-agent/src/runtime/services](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services)

How to carry this out:

1. Keep the envelope shape layered and future-onion-compatible, but only introduce it after current operations already run on `Establish`, `Move`, and `Hold`.
2. Use [crates/aura-agent/src/runtime/services/social_manager.rs](/Users/hxrts/projects/aura/crates/aura-agent/src/runtime/services/social_manager.rs) only as a Neighborhood/WoT permit-candidate input, not as the move executor or selection owner.
3. Keep transparent mode explicitly non-production and use it to validate controller behavior, path diversity, cover traffic, and packet-shape assumptions before encryption.

### 5.1 Define transparent move-envelope layout

- [ ] Add a transparent envelope format that matches the intended final packet structure
- [ ] Ensure it can carry:
      move traffic
      held-object deposit traffic
      selector-based retrieval traffic
      deferred-delivery and cache-seeding traffic where appropriate
- [ ] Keep the layout onion-identical in shape but transparent in content

**Success**: one envelope family can exercise the service architecture in simulation.

### 5.2 Quarantine transparent mode

- [ ] Gate transparent mode behind `#[cfg(feature = "transparent_onion")]`
- [ ] Exclude it from default and release production lanes
- [ ] Keep harness and shared-flow conformance lanes independent of it
- [ ] Add build or CI checks that transparent mode is exercised only in explicit debug, test, or simulation lanes

**Success**: no parity-critical production path depends on insecure headers.

### 5.3 Implement `LocalHealthObserver`

- [ ] Add a runtime-owned `LocalHealthObserver` in `aura-agent`
- [ ] Measure only local signals:
      reachable provider count
      RTT
      loss
      traffic volume
      churn
      observed route diversity
      queue or relay pressure
      hold success or failure
      sync opportunity frequency
- [ ] Smooth measurements with documented EMA, hysteresis, and rate-limit policy
- [ ] Do not create shared health consensus or journal truth

**Success**: the runtime emits stable local health snapshots without global coordination.

### 5.4 Implement local selection policy

- [ ] Add a runtime-owned selection service that consumes:
      local health snapshots
      Neighborhood-plane and Web-of-Trust-plane permit-scored candidates
      service descriptors from the registry
      protocol constants
- [ ] Produce typed local selection profiles
- [ ] Keep trust evidence provenance separate from any locally derived coarse tier or weighting bucket
- [ ] Treat profiles as continuous targets, not discrete regime labels
- [ ] Define bounded scheduling classes over the shared `Move` substrate:
      sync-blended
      bounded-deadline reply
      synthetic cover
- [ ] Ensure `LocalRoutingProfile::passthrough()` remains a valid profile: direct routing with no mixing, cover, or delay
- [ ] Define the fixed adaptive policy structure: constants, minimums, targeting curves, and gain parameters that the production controller operates within
- [ ] Ensure that once the privacy policy ships, production builds use only the fixed policy — direct parameter adjustment is development-only
- [ ] The output of Phase 6 simulation tuning fills in the concrete values for the fixed policy; until then, passthrough is the production default
- [ ] Include bounded profile-change rate and hysteresis to reduce thrash
- [ ] Include bounded provider-residency and anti-stickiness rules so high-confidence trust inputs do not become stable graph proxies
- [ ] Count application traffic and sync-blended retrieval before computing the remaining synthetic cover gap; measure accountability replies separately in the first deployment
- [ ] Add per-message-class constraints so ceremony and consensus flows can override general privacy depth
- [ ] Remove legacy selection code that still lives in non-runtime layers once the runtime selection owner is established

**Success**: final service selection is runtime-owned and continuous.

### 5.5 Implement path, move, and hold weighting

- [ ] Sample candidate providers with `RandomEffects`
- [ ] Add weighting inputs for trust evidence provenance, local health, diversity constraints, hold quality, and availability opportunity
- [ ] Ensure selection does not collapse to a single deterministic path or provider
- [ ] Add tests for soft preference without total concentration
- [ ] Add tests proving anti-stickiness still holds when one plane contributes much stronger candidates than the other

**Success**: the runtime can prefer strong candidates without destroying diversity.

### 5.6 Implement cover traffic as a runtime-owned service

- [ ] Add `CoverTrafficGenerator` in `aura-agent`
- [ ] Support both activity-cover baseline and mixing-mass gap filling
- [ ] Keep cover structurally indistinguishable from real traffic on the shared envelope family where applicable
- [ ] Compute synthetic cover demand only after accounting for application traffic and sync-blended retrieval in the first deployment
- [ ] Measure accountability-reply volume for later evaluation, but do not let it reduce the first-deployment cover floor
- [ ] Add a dedicated budget floor that real traffic cannot consume
- [ ] Add tests for budget isolation and non-zero floor behavior

**Success**: cover traffic has a real resource model and cannot be starved accidentally.

### 5.7 Enforce selection/runtime-locality rules

- [ ] Add proc-macro or typed-boundary annotations for selection-owned runtime services and their sanctioned query/ingress APIs
- [ ] Add lint or compile-fail coverage proving that selection outputs remain runtime-local and are not published as authoritative shared objects
- [ ] Add a thin script check only if needed to ensure new selection services declare their ownership and cache policy in `ARCHITECTURE.md`
- [ ] Remove temporary exported selection-state helpers once sanctioned runtime-local query surfaces are in place

**Success**: selection heuristics remain local runtime state by construction.

### 5.8 Phase 5 legacy cleanup

- [ ] Re-run the legacy sweep for old selection ownership paths, pre-envelope movement branches, and any cover-generation helpers that bypass the shared `Move` substrate
- [ ] Delete transitional selection-state exports, scheduler adapters, and transparent-envelope scaffolding that are no longer required after the runtime selection owner is stable
- [ ] Remove stale assumptions that accountability traffic or retrieval traffic use separate transport families when the shared envelope family is in place
- [ ] Record any remaining pre-adaptive-policy routing code with an explicit owner and removal phase before validation completes

### Phase 5 verification and closeout

- [ ] `cargo check -p aura-effects -p aura-agent -p aura-social -p aura-protocol -p aura-sync`
- [ ] `cargo test -p aura-effects -p aura-agent -p aura-social -p aura-protocol -p aura-sync --features transparent_onion`
- [ ] `just check-arch`
- [ ] `just lint-arch-syntax`
- [ ] `just ci-ownership-policy`
- [ ] Update docs and any affected crate `ARCHITECTURE.md` files for transparent onion envelopes, adaptive runtime policy, runtime selection ownership, and Neighborhood/WoT permit composition, following [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)
- [ ] Stage non-gitignored files touched in this phase
- [ ] Commit with a focused message, for example:
      `git commit -m "adaptive-privacy: add transparent onion envelopes and local selection"`

## Phase 6 — Validate And Tune

Goal: validate the family model before treating it as stable protocol behavior.

Reference:
- [Adaptive Runtime Policy](./adaptive_privacy_3.md#9-adaptive-runtime-policy)
- [Accountability Traffic As Ambient Mass](./adaptive_privacy_3.md#accountability-traffic-as-ambient-mass)
- [Sync-Blended Retrieval](./adaptive_privacy_3.md#95-sync-blended-retrieval)
- [Accountability and Limits](./adaptive_privacy_3.md#10-accountability-and-limits)
- [Open Questions and Validation Obligations](./adaptive_privacy_3.md#13-open-questions-and-validation-obligations)

How to carry this out:

1. Add scenarios against the existing simulator/runtime test infrastructure rather than inventing a one-off benchmark harness.
2. Prioritize one scenario for the current leak shape: the same social topology under different permit/select policies should not require interface-schema changes.
3. Explicitly compare neighborhood-only and WoT-assisted candidate pools to verify that trust-derived selection changes do not make the social graph observable from service shape.
4. Archive tuned constants and their rationale alongside the plan or in adjacent docs so the evidence survives the implementation sequence.

### 6.1 Extend simulator support

- [ ] Add simulator support for:
      establish flows
      move batching
      local health observation
      cover traffic
      bounded-deadline accountability replies
      route diversity
      partition and heal events
      provider saturation
      held-object retention
      selector-based retrieval
      sparse versus heavy sync opportunities
      move-to-hold cache seeding

**Success**: the simulator can exercise the family model rather than only relay delivery.

### 6.2 Define validation scenarios

- [ ] Add scenario matrices for:
      small, medium, and large reachable sets
      clustered and partitioned topologies
      provider saturation
      churn spikes
      low organic traffic and high cover dependence
      sparse sync opportunities
      deferred delivery under weak connectivity
      distributed cache seeding and recovery
      ceremony traffic under latency constraints
- [ ] Add observer-model scenarios where an adversary tries to infer:
      home membership from holder choice and timing
      direct-friend edges from route reuse and path residency
      introduction provenance from repeated provider selection patterns
- [ ] Add boundary scenarios near controller knees and rotation thresholds to detect oscillation or pathological selector churn
- [ ] Record metrics for:
      controller convergence
      oscillation and thrash
      delivery latency and loss
      batch size distribution
      replay and drop rates
      cover spend
      synthetic-cover gap after accounting for application traffic and sync-blended retrieval
      accountability-reply volume and timing distribution
      retrieval success and delay
      hold success and retention quality
      anonymity or correlation proxies
      home-membership inference precision/recall
      direct-friend-edge inference precision/recall
      introduction-provenance inference precision/recall
      reply-timing correlation precision/recall

**Success**: the plan tests stability, privacy tradeoffs, retrieval behavior, and held-object performance together.

### 6.3 Tune constants only after evidence

- [ ] Treat delay bounds, cover baselines, selector-rotation cadence, retention windows, and controller gains as provisional
- [ ] Replace brittle thresholds with smoother targeting where evidence says possible
- [ ] Write tuned constants and rationale back into docs and code comments
- [ ] If simulation disproves an assumption, update the proposal before continuing

**Success**: constants are evidence-backed rather than prose-backed.

### 6.4 Institutionalize enforcement for go-forward work

- [ ] Consolidate the new proc macros, lints, compile-fail tests, and thin script gates into the default contributor workflow
- [ ] Document which rules are type-enforced, lint-enforced, compile-fail-enforced, and script-enforced
- [ ] Add a go-forward checklist so any new service family composition or service-surface type must include the relevant declarations and tests in the same change
- [ ] Remove migration-only allowlists, exceptions, or compatibility notes once the new boundaries are stable

**Success**: architectural adherence is not just migration work; it becomes part of the normal development contract.

### 6.5 Phase 6 legacy cleanup

- [ ] Re-run the full migration sweep against the simulator, validation fixtures, and docs so old terminology or stale assumptions are not preserved in evidence artifacts
- [ ] Delete temporary measurement hooks, one-off scenario shims, or tuning-only compatibility paths that are not needed for the fixed-policy design
- [ ] Remove obsolete rationale text if simulation disproves or supersedes an earlier migration assumption
- [ ] Record any deferred cleanup that must survive into encryption work with an explicit owner and removal condition

### Phase 6 verification and closeout

- [ ] `cargo test -p aura-simulator`
- [ ] Run the adaptive privacy simulation matrix and archive results
- [ ] Update `work/adaptive_privacy_3.md` if evidence changes assumptions
- [ ] `just check-arch`
- [ ] `just lint-arch-syntax`
- [ ] Update any affected docs, reports, and rationale sections with tuned constants and validated assumptions, following [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)
- [ ] Stage non-gitignored files touched in this phase
- [ ] Commit with a focused message, for example:
      `git commit -m "adaptive-privacy: validate establish move hold model"`

## Phase 7 — Encrypt The Move Envelopes

Goal: replace transparent move envelopes with real layered encryption while preserving the already-validated abstraction boundaries. This is the final deployment stage: the tuned fixed policy from Phase 6 ships with encrypted envelopes, replacing the passthrough policy that has been running in production since the Phase 4 checkpoint.

Reference:
- [Relay](./adaptive_privacy_3.md#73-relay)
- [Move Objects](./adaptive_privacy_3.md#82-move-objects)
- [Reply-path accountability under onion routing](./adaptive_privacy_3.md#reply-path-accountability-under-onion-routing)
- [Accountability and Limits](./adaptive_privacy_3.md#10-accountability-and-limits)
- [Key Invariants](./adaptive_privacy_3.md#12-key-invariants)

How to carry this out:

1. Keep the envelope shape stable and swap only the transparent peel/read behavior for encrypted peel/read behavior.
2. Do not let encrypted movement reintroduce route policy or social-role-specific fields into the shared types.
3. Re-run the same worked examples and simulation scenarios after encryption so the abstraction remains intact, not just the transport behavior.

### 7.1 Implement route-layer hop crypto

- [ ] Add hop-crypto primitives in `aura-effects`
- [ ] Use route-layer or move-surface public keys from descriptors
- [ ] Derive per-route ephemeral hop secrets
- [ ] Keep recipient or context semantic crypto distinct from route-hop crypto
- [ ] Add tests for correct peel and forward behavior and inability of intermediate hops to inspect deeper layers

**Success**: layered route crypto is implemented without collapsing semantic boundaries.

### 7.2 Replace transparent movement with encrypted peeling

- [ ] Keep the same envelope shape and accountable movement semantics from earlier phases
- [ ] Swap transparent next-hop reads for encrypted peel operations
- [ ] Ensure move, hold, retrieval, and cover traffic use the same encrypted envelope family where appropriate
- [ ] Add end-to-end tests in encrypted mode
- [ ] Remove legacy transparent-only helper branches from production code paths once encrypted movement is validated

**Success**: encrypted mode is a drop-in replacement at the move-envelope boundary.

### 7.3 Retain transparent mode only as an explicit dev and simulation tool

- [ ] Keep `transparent_onion` only for debugging and simulation
- [ ] Verify production builds exclude it
- [ ] Verify harness and conformance lanes do not depend on it implicitly
- [ ] Remove accidental production references to transparent types, helpers, or feature gates

**Success**: transparent mode remains quarantined.

### 7.4 Phase 7 legacy cleanup

- [ ] Re-run the legacy sweep for transparent-only helper branches, pre-encryption peel/read paths, and any debug-only packet handling that still leaks into production code
- [ ] Delete obsolete transparent-movement helpers from release paths once encrypted peeling is canonical
- [ ] Remove stale documentation, comments, and fixtures that still describe transparent mode as anything other than an explicit dev/simulation tool
- [ ] Record any surviving debug-only transparent hooks with explicit quarantine boundaries and ownership

### Phase 7 verification and closeout

- [ ] `cargo check --workspace`
- [ ] `cargo test --workspace`
- [ ] `cargo test --workspace --features transparent_onion`
- [ ] `cargo build --release`
- [ ] `just check-arch`
- [ ] `just lint-arch-syntax`
- [ ] `just ci-ownership-policy`
- [ ] Update docs and any affected crate `ARCHITECTURE.md` files for encrypted move envelopes and the final post-transparent boundary, following [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)
- [ ] Stage non-gitignored files touched in this phase
- [ ] Commit with a focused message, for example:
      `git commit -m "adaptive-privacy: encrypt move envelopes"`

## Final Verification

- [ ] Re-run targeted crate tests touched across phases
- [ ] Re-run simulation evidence collection on the encrypted design
- [ ] Re-run:
      `just check-arch`
      `just lint-arch-syntax`
      `just ci-ownership-policy`
- [ ] Re-run the full legacy-term and legacy-object `rg` sweep and confirm all tracked matches are either removed or explicitly allowlisted with owner and removal rationale
- [ ] If harness or browser observation surfaces changed, run the relevant harness lanes and matrix checks
- [ ] Run a final legacy-removal audit:
      no duplicate descriptor caches
      no legacy mailbox polling paths
      no social-role-shaped service interfaces
      no canonical shared trust-tier enums that expose WoT-specific policy classes
      no transitional dual-write or dual-selection paths
      no stale migration allowlists or compatibility shims without an explicit owner and removal rationale
- [ ] If a scripted sweep helper was added, run it in CI/default contributor lanes so future legacy term reintroduction fails fast
- [ ] Perform a final documentation sync pass across `work/`, `docs/`, and affected crate `ARCHITECTURE.md` files, and ensure all updated prose adheres to [style_guide_docs.md](/Users/hxrts/projects/aura/work/style_guide_docs.md)
- [ ] Stage the remaining non-gitignored files
- [ ] Make the final integration commit, for example:
      `git commit -m "adaptive-privacy: integrate establish move hold model"`

## Risks And Enforcement

Reference:
- [Why This Model](./adaptive_privacy_3.md#25-why-this-model)
- [Policy Surface Mapping To Aura](./adaptive_privacy_3.md#6-policy-surface-mapping-to-aura)
- [Plane Ownership In Implementation](./adaptive_privacy_3.md#plane-ownership-in-implementation)
- [Accountability Traffic As Ambient Mass](./adaptive_privacy_3.md#accountability-traffic-as-ambient-mass)
- [Key Invariants](./adaptive_privacy_3.md#12-key-invariants)

### Main Risks

- over-generalization before the first concrete service boundary stabilizes
- duplicate cache ownership between Layer 5 services and `aura-agent`
- social-role vocabulary leaking back into descriptor and object types
- Web-of-Trust evidence leaking into observable route shape, retrieval shape, or retention behavior
- runtime-local trust tiers accidentally becoming canonical shared enums or descriptor fields
- `Hold` accidentally absorbing local indexing and cache-view concerns
- failing to keep the shared `Hold` substrate separate from the `DeferredDeliveryHold` and `CacheReplicaHold` profile semantics
- selection heuristics being mistaken for shared authoritative state
- direct-friend or FoF trust being materialized as authority-shaped objects instead of relational contexts
- friends-of-friends becoming canonical shared graph state rather than bounded local derivation or introduction evidence
- deploying Hold with a non-selector retrieval pattern that must be migrated when privacy ships
- passthrough production deployment masking interface problems that only surface when the adaptive policy activates
- tiered retention by social distance creating a side channel that narrows the anonymity set under onion routing
- Hold GC diverging from journal GC vocabulary, resulting in two unrelated expiration/invalidation systems
- neighborhood-scoped Hold overwhelming individual node storage budgets if capacity is not calibrated for the wider scope
- rotating holder subsets collapsing into sticky low-diversity selections that undermine the intended anonymity set
- neighborhood-budgeted reciprocity remaining too weakly specified to shape runtime selection and admission in practice

### Enforcement Strategy

- declare service-definition templates in docs and `ARCHITECTURE.md`
- prefer typed family/object boundaries over naming convention alone
- add compile-fail or lint-style checks where interfaces drift toward social-role-specific type shapes
- add coverage proving Neighborhood and Web-of-Trust permit classes do not become wire-visible service classes
- add coverage proving trust evidence provenance and runtime-local selection tiers stay distinct types
- keep `aura-agent` as the canonical owner for local caches and selection state
- add regression tests specifically aimed at the current traffic-pattern leak shape
- prefer proc macros, Rust-native lints, and compile-fail tests over grep-heavy shell checks whenever the rule can be expressed in the type system or AST
- keep shell scripts thin and governance-oriented: synchronization, required declarations, and lanes that cannot be proven cleanly in Rust alone
