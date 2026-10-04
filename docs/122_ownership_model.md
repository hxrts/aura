# Ownership Model

This document defines Aura's ownership model for authority, mutation, async lifecycle, and terminal failure behavior across the workspace.

It complements [System Architecture](001_system_architecture.md) for the high-level system view. See [Effect System](103_effect_system.md) for effect boundaries. See [Runtime](104_runtime.md) for lifecycle and supervision. See [Project Structure](999_project_structure.md) for layer placement.

## Overview

Aura uses four ownership categories: `Pure`, `MoveOwned`, `ActorOwned`, and `Observed`. Every parity-critical subsystem, operation, and mutation surface must fit one of these categories. Bugs of the form "multiple layers own the same truth" are architectural violations.

## Categories

### `Pure`

`Pure` code is deterministic and effect-free. Use it for reducers, validators, state machines, fact interpretation, and typed contracts. `Pure` code may not own long-lived mutable async state, publish semantic lifecycle, or rely on ambient authority.

### `MoveOwned`

`MoveOwned` code represents exclusive authority through consumed values. Use it for operation handles, owner tokens, delegation records, session handoff objects, and stale-owner invalidation boundaries.

Ownership transfer must consume a handoff object or owner token. Stale holders must become invalid by construction. Direct owner-field rewrites are forbidden where a transfer object is required.

### `ActorOwned`

`ActorOwned` code owns long-lived mutable async state under one live task. Use it for runtime services, supervisors, maintenance loops, readiness coordinators, lifecycle coordinators, and command ingress loops.

There must be exactly one live owner for the mutable state domain. Mutation happens through typed ingress, not shared mutable access. Long-lived background work must be supervised. Owner death must lead to explicit terminal state, failure, or shutdown.

In practice, Aura's production `ActorOwned` runtime path is `aura-agent`:

- bounded ingress is declared with `aura-core::BoundedActorIngress`
- long-lived service ownership is internal to runtime service modules
- shared actor handles/mailboxes are crate-private runtime internals, not a
  public API for higher layers
- raw spawn lives only inside the sanctioned supervision implementation
- public runtime facades must consume shared runtime-owned supervisors and
  ceremony runners. They may not allocate private ownership roots as a
  convenience constructor.
- service health must reflect degraded obligation progress explicitly. "Task
  exists" is not a sufficient health contract when required maintenance work is
  failing.

### `Observed`

`Observed` code reads and presents authoritative state but does not own it. Use it for projections, UI rendering, harness reads, diagnostics, and reporting.

`Observed` code may submit typed commands to owner surfaces. It may not author semantic lifecycle or readiness truth. It may not repair ownership mistakes by mutating product state.

Observed/reactive code also may not synthesize canonical entity metadata from
weaker signals. If a channel, invitation, or similar parity-critical entity
requires canonical name/context materialization, one explicit owned path must
materialize it end to end. Membership events, UI projections, or view-local
fallbacks may enrich an already-materialized entity, but they may not create
or repair the canonical entity shape.

## Capability-Gated Authority

The ownership model builds on Aura's existing capability system. Parity-critical mutation and publication should be capability-gated.

Semantic lifecycle publication requires an appropriate capability. Readiness publication requires a coordinator-owned capability. Ownership transfer requires a transfer capability or sanctioned handoff token. Actor ingress that mutates owned state requires the actor's command boundary.

The goal is to make incorrect authority structurally hard to express. Code should not be able to publish semantic truth merely because it can call a helper.

The same fail-closed rule applies to authoritative signal reads in semantic
owner code. Missing or unavailable authoritative state must surface as explicit
failure or degraded state, not `Default::default()` business truth.

## Usage Examples

### When To Use `Pure`

Use `Pure` when the code interprets values rather than owning authority or async lifecycle.

```rust
pub fn reduce_membership(
    current: MembershipState,
    fact: MembershipFact,
) -> MembershipState {
    match fact {
        MembershipFact::Joined { member } => current.with_member(member),
        MembershipFact::Left { member } => current.without_member(member),
    }
}
```

This stays `Pure` because it consumes and returns values, owns no long-lived state, and does not publish lifecycle directly.

### When To Use `MoveOwned`

Use `MoveOwned` when stale access must become invalid after handoff.

```rust
use aura_core::{
    issue_owner_token, OwnershipTransferCapability,
};

let capability = OwnershipTransferCapability::new("ownership:transfer");
let token = issue_owner_token(&capability, "invite-op-7", "channel:alpha");
let transfer = token.handoff("invite-coordinator");
```

The original `token` is consumed by `handoff`. Trying to act through the old owner is a compile-time error.

Typed ownership capabilities from the same wrapper family can also be issued onto Aura's existing Biscuit path without first down-converting them to raw `CapabilityKey` values via `ownership_capability_token_request_for(...)`. Lower layers should not expose parallel raw ownership-capability request helpers once a typed wrapper family exists.

### When To Use Capability Tokens

Use capability wrappers whenever parity-critical code needs authority to author semantic truth.

```rust
use aura_core::{
    issue_operation_context, AuthorizedProgressPublication,
    AuthorizedReadinessPublication, LifecyclePublicationCapability,
    OperationContextCapability, OperationTimeoutBudget, OwnedShutdownToken,
    OwnerEpoch, PublicationSequence, ReadinessPublicationCapability,
    TraceContext,
};

let context_capability = OperationContextCapability::new("semantic:context");
let lifecycle_capability = LifecyclePublicationCapability::new("semantic:lifecycle");

let mut ctx = issue_operation_context(
    &context_capability,
    "send_message",
    "send_message-7",
    OwnerEpoch::new(0),
    PublicationSequence::new(0),
    OperationTimeoutBudget::deferred_local_policy(),
    OwnedShutdownToken::detached(),
    TraceContext::detached(),
);

let update: AuthorizedProgressPublication<_, _, _, _> =
    ctx.publish_progress(&lifecycle_capability, "waiting");
let terminal = ctx
    .begin_terminal::<(), &'static str>(&lifecycle_capability)
    .fail("timeout");
```

Context minting and publication both require capability-shaped inputs. Random helper code cannot fabricate owner context or publish lifecycle by accident.

The same rule applies to readiness and actor-ingress mutation. Higher layers should prefer `AuthorizedReadinessPublication<T>` and `AuthorizedActorIngressMutation<T>` over raw capability arguments when they need to move parity-critical authority across API boundaries.

### When To Use `ActorOwned`

Use `ActorOwned` when one live task must own mutable async state and terminal responsibility.

```rust
struct ChannelInviteCoordinator {
    pending: HashMap<InviteId, InviteState>,
    rx: mpsc::Receiver<InviteCommand>,
}

impl ChannelInviteCoordinator {
    async fn run(mut self) {
        while let Some(command) = self.rx.recv().await {
            self.apply(command).await;
        }
    }
}
```

There is one live owner of `pending`. Mutation happens through typed ingress. Owner drop is a lifecycle event that must be surfaced explicitly.

## Selection Heuristics

Choose `Pure` first if the logic can be expressed as value-in/value-out. Choose `MoveOwned` when the hard problem is exclusive authority or stale-holder invalidation. Choose `ActorOwned` when the hard problem is long-lived mutable async state under one live owner. Choose `Observed` only for read-only presentation or diagnostics.

Anti-patterns to avoid:

- a shell callback publishing semantic success (should be `Observed`)
- shared mutable `Arc<Mutex<_>>` state spread across tasks (should be `ActorOwned`)
- rewriting an owner field in place after delegation (should be `MoveOwned`)
- reducers that call time/network/storage directly (no longer `Pure`)
- reactive/view code inventing channel names from raw ids or membership events
  instead of consuming an owned canonical materialization path
- runtime or workflow code mining pending invitations, optimistic sketches, or
  cross-context routing cache entries to repair a missing authoritative
  binding/context after handoff

## Contributor Requirement

New or materially changed parity-critical modules must declare their ownership
category in the crate `ARCHITECTURE.md`.

That declaration must name:

- the ownership category (`Pure`, `MoveOwned`, `ActorOwned`, or `Observed`)
- the authoritative owner for terminal lifecycle if the surface is async
- the capability-gated mutation/publication points
- the local timeout/backoff owner if deadlines or retries are involved

The ownership declaration is part of the change, not optional follow-up documentation.

## Terminality

Every parity-critical operation must have typed terminal behavior. Direct boundaries use `Result<T, E>`. Long-running operations use typed lifecycle phases: `Submitted`, zero or more intermediate phases, then `Succeeded`, `Failed(E)`, or `Cancelled`.

Runtime-owned bridge APIs must follow the same rule. If a public runtime call can
distinguish `no progress`, `started`, `already running`, `processed`,
`degraded`, or `mutated`, it must return a typed outcome instead of collapsing
those states into `Result<(), E>`.

Every submitted operation must reach a terminal state. Terminal states may not regress. Owner drop must publish failure or cancellation explicitly.

Device enrollment setup verification and code issuance belong to one submission
owner. Issuance success requires the runtime's actual ceremony result; common
preparation must not publish a second success. This start proof does not prove
enrollment completion, which has its own operation instance and authoritative
ceremony outcome. Invalid possession signatures or bindings yield
`PermissionDenied`; malformed or out-of-validity setup statements yield
`InvalidArgument`. Bounded deadlines yield `OperationTimedOut` with the original
typed cause retained through the standard error source chain.

Terminality alone is not strong enough. Aura also requires owner-internal liveness: a legal owner may not contain unbounded internal work that can keep an operation in `OperationState::Submitting` forever. If the owner can hang indefinitely while still technically being the "right" owner, the architecture is incomplete.

Timeout-triggered returns do not relax this rule. A timeout may fail an
operation, but it may not silently convert an ambiguous owner-internal state
into nominal success. Typed `NoProgress` or `Degraded` outcomes are preferable
to unit success when runtime-owned work can observe those distinctions.

## Semantic Owner Protocol

Parity-critical semantic operations must follow one protocol from submission to
terminal publication.

1. A frontend or harness may create a local submission record.
2. If app/runtime workflow ownership is required, the frontend/harness must
   hand ownership off immediately before the first awaited workflow step.
3. After handoff, only the canonical owner may publish non-local lifecycle.
4. The canonical owner must publish a terminal state before any best-effort
   repair, warm-up, or post-success reconciliation that is allowed to fail.
5. Best-effort follow-up work must never be required for the operation to stop
   being `OperationState::Submitting`.

This forbids the bug shape where:

- the callback is the "temporary" owner
- the app workflow is the "real" owner
- both are structurally legal
- but the callback keeps local `OperationState::Submitting` state alive while
  the workflow has
  already reached terminal publication or is blocked in best-effort work

The handoff boundary must therefore be before awaited workflow execution, not after it.

Macro declaration rule:

- `#[aura_macros::semantic_owner(owner = "...", terminal = "...", category = "move_owned")]`
  is the sanctioned declaration surface for move-owned semantic workflow
  boundaries
- semantic owners must also declare:
  - `postcondition = "..."` for the authoritative state guaranteed by success
  - `depends_on = "a,b,..."` for prerequisite readiness edges
  - `child_ops = "a,b,..."` for sanctioned semantically required child work
  - `proof = Type` whenever success is only valid if a typed postcondition
    witness has been established
- `#[aura_macros::capability_boundary(category = "capability_gated", capability = "...")]`
  is the sanctioned declaration surface for capability-bearing mint/publication
  helpers
- `#[aura_macros::actor_owned(owner = "...", domain = "...", gate = "...", command = Type, capacity = N, category = "actor_owned")]`
  is the sanctioned declaration surface for long-lived actor-owned async domains
- `#[aura_macros::ownership_lifecycle(initial = "...", ordered = "...", terminals = "...")]`
  is the sanctioned declaration surface for small parity-critical lifecycle
  enums
- `#[aura_macros::authoritative_source(kind = "...")]` is the sanctioned
  declaration surface for helpers that mint or read authoritative semantic
  truth. Valid kinds are `runtime`, `signal`, `app_core`, and `proof_issuer`.
- `#[aura_macros::strong_reference(domain = "...")]` is the sanctioned
  declaration surface for canonical strong-reference carriers. Valid domains are
  `channel`, `invitation`, `ceremony`, `home`, and `home_scope`.
- `#[aura_macros::weak_identifier(domain = "...")]` is the sanctioned
  declaration surface for weak identifier carriers that must not be upgraded
  into strong bindings without an explicit owner path

## Reactive Contract

Parity-critical reactive consumers must rely on one explicit subscription contract.

- subscription to an unregistered signal is a typed failure, not an empty or inert stream
- there is no implicit registration wait for parity-critical consumers
- if a subscriber lags behind the broadcast buffer, the handler logs the lag and resumes from a newer snapshot
- parity-critical owners may not infer replay or lossless history from the reactive layer unless an explicit replay contract exists
- hook ownership is explicit across installation: an `Installing` group cannot be reported ready, a `Ready` group owns one attached stream and acknowledged listener per required signal, and `Stopped` cancels those streams on detach or failed installation
- a mounted frontend owns one supervised observer group per runtime generation and one receiver per distinct required signal; its health records attachment, closure, and recovery failures as typed states
- listener tasks may retain an owned task spawner but must not retain the group lifetime owner; unmount or generation replacement cancels all listeners before another group becomes ready
- receiver attachment precedes the current-state read; after reattachment or lag, the observer derives state from a fresh current snapshot rather than assuming intermediate updates were replayed

This means reactive delivery is a transport for authoritative snapshots, not an alternate owner of semantic truth. Owner code must tolerate "newer snapshot after lag" semantics without silently treating a missed update as "no change."

Enforcement rule:

- ownership declarations, strong-reference markers, and authoritative-source
  markers should be enforced first by proc-macro validation, Rust-native lints,
  and compile-fail tests
- shell scripts should remain only for integration checks or governance rules
  that are not realistically provable in types or Rust-native syntax analysis

Reactive/view consumers also may not fabricate canonical entities from partial
facts. For example, a membership fact may update membership for a known channel,
but it may not create a channel with `channel_id.to_string()` as a fallback
name. Canonical entity materialization must come from one owned path that
already carries the authoritative metadata.

The materialization owner exposes a distinct opaque creation witness for each
entity family. It mints that witness only from a validated creation fact or an
authenticated completion outcome with the same required metadata. A
projection's insertion boundary requires the corresponding witness;
enrichment boundaries require an already-materialized entity and cannot mint
one. Witness constructors remain private to the materialization owner.
Out-of-order enrichment is retained for later application only when its entity
identity and context match the eventual creation evidence. Compile-fail/API
tests guard the constructor boundary, and permutation/replay tests guard the
order-independent behavior.
The app-private `HomeCreationWitness` constructor accepts only `HomeCreated`
shape through `ProjectionOwner::home_created_witness`. The projection owner
controls that conversion, while journal commitment remains the responsibility
of the runtime ingestion path. A plain public fact value does not itself prove
commitment.

## Owner Body Rules

Once a function is designated as a semantic owner, its body is constrained more strictly than ordinary async code.

Allowed:

- bounded awaits through approved timeout-budget helpers
- retries through approved retry-policy helpers
- publication through capability-gated lifecycle/readiness helpers
- explicit handoff to another sanctioned owner

Forbidden:

- raw open-ended `.await` on runtime/effect calls
- awaiting best-effort network or transport side effects before terminal
  publication
- retaining a frontend-local owner while awaiting an app-owned workflow
- detached work that still owns terminal responsibility
- direct spawn from a semantic owner except through an explicitly declared
  child-operation surface
- silently discarding parity-critical results or errors
- ad hoc local retries, sleeps, or polling loops

If a semantic owner needs long-lived convergence, that convergence must be owned by a dedicated `ActorOwned` coordinator and expressed as typed readiness or typed terminal lifecycle, not as an unbounded await hidden inside a helper.

## Typed Success Proofs

Declared postconditions are not documentation-only. For parity-critical operation families, `Succeeded` should be tied to an opaque typed proof surface whenever the authoritative postcondition is stronger than "the function returned successfully".

The required pattern is:

- capability-gated code performs the authoritative mutation, readiness check,
  or materialization step
- that sanctioned helper mints an opaque proof witness for the declared
  postcondition, such as a channel-membership-ready proof
- the semantic owner publishes terminal success by consuming that proof through
  the canonical success path

This is intentionally different from a capability token:

- a capability answers who is allowed to act
- a proof answers what has become true

Proofs must therefore be minted by capability-gated code, but the proof itself must not be the authority token.

The canonical direction is:

- `#[aura_macros::semantic_owner(..., postcondition = "...", proof = Type)]`
- owner success goes through `publish_success_with(proof)` or the equivalent
  canonical proof-bearing success helper
- plain `publish_phase(Succeeded)` is forbidden for proof-bound owners

Proof constructors stay private. External code must not be able to forge a proof witness, and the compile-fail suites should prove that boundary.

## Best-Effort Separation

Aura distinguishes *terminally required work* from *best-effort work*.

Terminally required work:

- determines whether the operation is `Succeeded`, `Failed`, or `Cancelled`
- may block terminal publication only through bounded waits owned by the
  canonical semantic owner

Best-effort work:

- may improve projection quality, connectivity, warming, discovery, or local
  convenience
- must run only after terminal publication, or under a different owner with its
  own explicit lifecycle
- must not prevent the submitted operation from leaving
  `OperationState::Submitting`
- must not directly publish parity-critical lifecycle or readiness
- must not directly perform parity-critical mutation such as committing facts,
  materializing authoritative state, registering required ownership, or other
  work that a later parity-critical operation depends on
- must not use the `best_effort_*` naming surface unless they actually obey the
  best-effort contract above. Aura treats that prefix as a reserved ownership
  boundary and lints it accordingly even when the helper forgot to add an
  explicit `#[best_effort_boundary]`.

If a step mutates authoritative state required by a later semantic operation, it is not best-effort. It belongs either:

- inside the canonical semantic owner before `Succeeded`, or
- inside a distinct owned child operation with its own explicit lifecycle and dependency edge

This rule is stronger than "use timeouts". A bounded best-effort step is still architecturally wrong if it owns the primary operation's terminal state.

## Correct-By-Construction Requirements

For parity-critical operation families, "correct by construction" means:

- submission uses one canonical typed owner wrapper
- owner handoff uses one canonical consumed transfer API
- terminal publication uses one capability-gated API family
- success implies one declared authoritative postcondition
- proof-bound success consumes one opaque typed proof minted by sanctioned capability-gated code
- bounded awaits use one approved timeout-budget helper family
- retries use one approved retry-policy helper family
- best-effort work uses one explicit helper family that cannot publish or delay primary terminal state
- semantic owners do not spawn except through declared child-operation APIs
- parity-critical results are not ignored or downgraded to logging-only paths
- an issued code and the ceremony it starts have distinct terminal owners and
  operation instances linked by ceremony ID; code issuance cannot publish
  completion success
- a ceremony owner accepts one typed terminal outcome and preserves the first
  result under duplicate, late, or replayed responses
- frontend operation snapshots retain the authoritative failure domain/code and
  cancellation state; local wizard progress does not publish ceremony success
- a frontend may return an import submission handle before it knows an
  invitation's type, but it hands the generic import owner to the app before
  the first awaited verification/cache step. Only a verified import result
  allocates the separate typed contact, guardian, or channel accept owner

## Enforcement Ratchet

Aura treats ownership enforcement as a ratchet, not a static checklist.

The desired order of strength is:

1. private constructors and opaque types
2. capability-gated APIs
3. canonical owner wrappers/macros
4. AST-backed lints
5. compile-fail tests
6. invariant and concurrency tests
7. thin CI shell wrappers that call those stronger checks

Shell scripts remain useful as workflow glue, but they are the weakest layer.
If a policy matters for parity-critical correctness, the long-term goal is to encode it in types, macros, or AST-backed analysis rather than rely on grep.

## Required Invariants For Parity-Critical Operations

Every parity-critical operation family should have invariant tests for all of the following:

- owner drop forces `Failed` or `Cancelled`
- terminal state cannot regress
- stale owner or stale handle cannot advance state
- canonical owner publishes terminal state within a bounded budget
- `Succeeded` implies the semantic owner's declared postcondition holds
- proof-bound `Succeeded` consumes the correct typed witness for that declared
  postcondition
- best-effort failure cannot block terminal publication
- no later parity-critical operation can depend on hidden best-effort work to
  make a successful operation "actually true"
- frontend-local submission state cannot mask authoritative terminal state after
  handoff
- older authoritative instances cannot overwrite newer local submissions


## Frontend Handoff Rule

Layer 7 frontends are primarily `Observed`, but they are allowed to own a very small local submission window. That window is subject to a strict rule:

- if the frontend owns terminal publication, it must settle locally
- if the app/runtime owns terminal publication, the frontend must relinquish
  local ownership before awaiting the app/runtime workflow

There is no supported middle state where the frontend keeps a local submitting record "just in case" while the canonical workflow runs elsewhere.

## Time And Ownership

Timeouts and backoffs are part of ownership, not incidental implementation detail.

For every parity-critical async path, the architecture must identify:

- who owns the deadline
- who owns retry policy
- whether the wait is terminally required or best-effort
- what happens when the budget is exhausted

Wall-clock time remains a local choice in Aura's time model, but timeout policy ownership is not a local choice. A path that can wait forever without a declared owner and terminal consequence is an ownership violation.

## Layer Guidance

Layer 1 (`aura-core`) defines the shared ownership vocabulary: primitives, typed lifecycle helpers, and capability boundaries.

Layer 2 (domain crates) defaults to `Pure`. Use `MoveOwned` only when transfer semantics are part of the domain itself. Domain crates should not silently grow runtime-style async ownership.

Layer 3 (implementation crates) defaults to stateless handlers. Avoid long-lived mutable ownership except for narrow adapter internals.

Layer 4 (orchestration) uses `MoveOwned` for delegation, session ownership, and handoff. Use `ActorOwned` for long-lived orchestration coordinators only.

Layer 5 (feature crates) should have single-owner semantic lifecycle. Wrappers, views, and shells must not co-author stronger semantics than canonical workflows.

Layer 6 (runtime) is the primary `ActorOwned` layer. Runtime services, caches, maintenance loops, and supervisors should be actor-owned. Ownership transfer still uses `MoveOwned` surfaces.

Layer 7 (interface) is primarily `Observed`. Frontends may provide command ingress mechanics but do not own parity-critical semantic truth.

Layer 8 (testing) may simulate actors and capabilities. Parity-critical lanes must observe and submit through the same ownership boundaries as production code.

## Workspace Ownership Inventory

This inventory covers every Rust crate under `crates/`. It is the workspace-level baseline. Detailed per-module inventories belong in crate `ARCHITECTURE.md` files.

### Layer Summary

- Layer 1 (`aura-core`) is primarily `Pure` and defines the canonical
  `ActorOwned`, `MoveOwned`, and capability-gated vocabulary. It does not own
  long-lived mutable runtime state.
- Layer 2 crates stay primarily `Pure`. They may expose `MoveOwned` records
  when transfer semantics are part of the domain model, but they do not grow
  runtime-style actor ownership.
- Layer 3 crates stay `Pure` and infrastructural. They implement handlers and
  composition without becoming semantic owners of parity-critical lifecycle.
- Layer 4 crates commonly mix `ActorOwned` coordinators and `MoveOwned`
  transport/protocol surfaces. Coordination publication and ingress remain
  capability-gated.
- Layer 5 crates mix `Pure`, `MoveOwned`, and narrow `ActorOwned` protocol
  coordinators. Ceremony, invitation, recovery, sync, and rendezvous flows use
  typed handles, typed terminal states, and coordinator-owned publication.
- Layer 6 splits strictly:
  - `aura-agent` is the production `ActorOwned` runtime and the only sanctioned
    production structured-concurrency path
  - `aura-app` is primarily `Pure` plus `MoveOwned` and owns authoritative
    semantic lifecycle/readiness publication for shared semantic flows
  - `aura-simulator` is `ActorOwned` for simulation coordination and `Observed`
    for test-facing exports
- Layer 7 crates are strict consumers:
  - `aura-terminal` and `aura-web` are `Observed` with narrow ingress/bridge
    ownership only
  - `aura-ui` is `Observed`
  - frontend-local parity-critical lifecycle ownership is not allowed outside
    the sanctioned local-terminal/handoff boundary
- Layer 8 crates are primarily `Observed`. Test-only actor helpers are allowed
  where they mirror production owner boundaries rather than inventing a
  separate semantic model.

Each crate `ARCHITECTURE.md` must classify its parity-critical modules, identify actor-owned domains, name consumed move-owned surfaces, and list the capability-gated mutation/publication points it exposes.

## Enforcement

The ownership model is enforced in layers.

Types and private constructors provide the first line of defense. Capability-gated mutation and publication APIs form the second layer. Canonical owner wrappers and macros provide the third layer. AST-backed checks, compile-fail tests, invariant tests, and then thin `scripts/check/*.sh` / `just ci-*` wrappers complete CI enforcement.

Enforcement split:

- Types, private constructors, sealed traits, and consumed ownership wrappers
  are the primary defense.
- Ownership and lifecycle declaration macros in `aura-macros` force explicit
  boundary classification.
- `trybuild` compile-fail suites in `aura-core` and `aura-app` prove that
  forbidden ownership/publication patterns do not compile.
- Parity-critical APIs must require the strongest available typed input.
  Once authoritative context exists, raw-id helper calls, `resolve_*`
  re-resolution, and `*_or_fallback` repair paths are ownership violations.
- Rust-native lint binaries in `aura-macros` provide syntax-level fences for:
  - proof-bound semantic owners using plain `Succeeded` publication instead of
    proof-bearing success
  - semantic owners publishing success and then launching detached continuation
  - semantic owners spawning outside explicit child-operation ownership
  - silent discard of parity-critical results and errors
  - best-effort boundaries performing direct parity-critical mutation or
    publication
  - helpers named `best_effort_*` being treated as real best-effort boundaries
    rather than advisory comments
  - raw spawn / raw task-handle escape hatches
  - frontend semantic handoff bypasses
  - authoritative-ref downgrade via raw-id re-resolution inside
    authoritative-only workflow slices
  - raw timeout/time-domain usage in protected modules
- Thin shell wrappers under `scripts/check/` remain only as CI glue or where
  the invariant is inherently integration-level rather than compile-time.
- Governance-only checks stay clearly separate from code-correctness
  enforcement:
  - `just ci-ownership-categories`
  - `just ci-harness-actor-vs-move-ownership`
  - Aura `toolkit/xtask` user-flow guidance sync check
- Runtime/integration checks remain appropriate for properties such as runtime
  shutdown ordering and instrumentation schema discipline because those are
  orchestration-level invariants, not just API-shape rules.

Primary enforcement belongs in typed ownership primitives and proc-macro declarations. For the frontend/harness stack this means:

- `HarnessUiOperationHandle` and `UiOperationHandle` are constructor/accessor
  surfaces, not public-field records
- readiness refresh helpers stay private to `aura-app::workflows`
- the local-terminal and workflow-handoff owner wrappers are the sanctioned
  frontend submission windows
- `UiTaskOwner` and `WebTaskOwner` are the only sanctioned frontend
  task-ownership surfaces

Shell checks such as semantic-owner bounded-await and frontend/harness boundary wrappers are secondary fences for narrow escape hatches, not the source of truth for semantic correctness.

Phase-6 rollup rule:

1. extend typed ownership primitives or proc-macro declarations first
2. add or update compile-fail coverage for the newly-closed misuse shape
3. place syntax-owned enforcement in `aura-macros` lint binaries and run it
   through `just lint-arch-syntax` or `just ci-ownership-policy`
4. keep shell backstops only for integration-governance checks that cannot be
   proved in types or Rust-native syntax analysis
5. delete the legacy helper, compatibility wrapper, migration shim, or stale
   test that the stronger contract replaced in the same milestone

Do not keep dormant fallback modules, compatibility constructors, or
"temporary" legacy upgrade paths after the strong contract exists. If a rule
can be enforced by types, macros, compile-fail suites, or Rust-native lints,
that enforcement is primary and the shell layer stays thin.

For proof-bearing postconditions specifically, the desired enforcement order is:

1. private proof constructors
2. capability-gated proof minting helpers
3. `#[semantic_owner(..., proof = Type)]`
4. compile-fail tests proving proofs cannot be forged or minted from the wrong
   module
5. AST-backed linting that rejects plain `Succeeded` publication in proof-bound
   owners
6. invariant tests proving the proof-minting helper is semantically honest

Scripts alone are not sufficient. The API must make the wrong pattern hard or impossible first.

### Service-Family Enforcement Map

For new `Establish`, `Move`, or `Hold` surfaces, keep the enforcement split
explicit:

- type-enforced:
  - runtime-local policy stays inside runtime-owned services
  - strongest typed reference continues after authoritative context exists
  - capability-gated mutation/publication boundaries
- proc-macro declaration-enforced:
  - `#[service_surface(...)]`
  - `#[actor_owned(...)]`, `#[semantic_owner(...)]`, or the narrower ownership declaration that matches the boundary
- compile-fail-enforced:
  - service-surface misuse and private-constructor misuse in `trybuild` suites
  - stale-owner / wrong-capability / wrong-layer misuse when the API shape can reject it
- lint-enforced:
  - `aura-macros` ownership and architecture lint binaries run through
    `just ci-ownership-policy` and `just lint-arch-syntax`
- script-enforced:
  - thin repo-wide integration/governance gates such as
    `just ci-service-surface-policy`,
    `just ci-service-registry-ownership`,
    `just ci-harness-ownership-policy`, and
    `just ci-adaptive-privacy-tuning`

The default contributor path for service-family boundary work is:

1. `just lint-arch-syntax`
2. `just ci-ownership-policy`
3. `just check-arch`
4. `just ci-adaptive-privacy-tuning` when the change affects adaptive privacy
   policy, simulator evidence, or control-plane parity artifacts

### Same-Change Checklist

Any new service-family composition or service-surface type must include, in the
same change:

1. the declaration macros for the boundary and owner category
2. typed/runtime-local state placement that keeps shared truth separate from local policy
3. compile-fail or invariant coverage for the strongest misuse shape the API can reject
4. lint or script wiring for the remaining syntax/integration boundary
5. doc updates in the affected crate `ARCHITECTURE.md` and any authoritative guide that owns the contract
6. removal of the superseded helper, adapter, allowlist, compatibility branch, or migration note

If item 6 cannot be completed immediately, record the owner and explicit
removal condition in the active migration inventory rather than leaving a silent
compatibility path behind.

## Review Checklist

When adding or modifying a parity-critical path, ask these questions:

- What category is this module or subsystem?
- Who is the single live owner of mutable async state?
- How is authority transferred?
- What capability authorizes mutation or publication?
- What is the typed terminal success/failure contract?
- What authoritative postcondition does `Succeeded` actually guarantee?
- Does success require a typed proof witness, and where is that proof minted?
- Where does local submission ownership end and canonical workflow ownership begin?
- Once authoritative context exists, which strong typed reference carries it
  through the rest of the flow?
- Could any later helper silently downgrade from that strong reference back to
  raw-id lookup, fallback, or re-resolution?
- Which awaits are terminally required, and which are best-effort only?
- Is any later parity-critical step relying on hidden best-effort follow-up?
- What bounded budget owns each required wait and retry?
If these answers are unclear, the design is not complete enough.

## Window ownership

The common window primitive is `Pure`: sealed physical-millisecond and receipt-generation domains provide checked bounds and half-open membership. Admission, progression, cryptographic provenance and durable acknowledgment belong to their domain owners. Arithmetic serialization cannot mint an admission capability.

A fixed physical execution window is `MoveOwned` at admission and shares one observation/exhaustion owner across clones and children. Its enclosing runtime is `ActorOwned`. An observed ceremony snapshot cannot mint that execution capability or replace its interval. The registered owner admits execution directly from its authoritative registry and retains the sealed allocation capability through subsequent checkpoints.

Initial allocation and live admission are distinct lifecycle phases. Only an internally minted pre-live capability can retain a missing initial checkpoint from the exact original secure allocation, before canonical registration or immutable live-boundary evidence exists. The live boundary is retained before execution can observe the registry entry. After that boundary, missing observation history fails closed. A required checkpoint holds allocation continuity through its write, retains monotone durable history and preserves sticky failures; cancellation or storage failure prevents the required continuation/publication. A terminal outcome for the same allocation does not invalidate historical acknowledgment or renew execution.

Supervised execution identity comes from successful bounded registry admission,
not from a caller identifier or an executor thread. Each registered future poll
and destructor has its original owned identity; nested scopes restore their
caller and cancellation cannot leave an ambient owner installed. Identity is
stable across suspension and executor migration and distinguishes sibling groups
with identical local task counters. Native and browser choreography session
bindings use that identity for supervised tasks. Identity observation grants no
ability to admit a task or enter an execution scope, and identity carries no
cryptographic peer, membership or signing authority.

### Enrollment notification ownership

An enrollment terminal-notice listener is a registered sibling in the existing
invitation service task tree. The original admitted window and physical-device
witness bind its admission; a cloned window shares observations, lease and
checkpoint and grants no new duration. The listener owns its actual VM session
slot across bounded selection. Explicit shutdown closes that slot before normal
completion; forced drop retires exact owner generation and scope without
claiming asynchronous close or signed terminal evidence. Required native causes
remain in the owning task tree's health and terminal failure observation.

### Cancellation observation custody

The authoritative tracker mints cancellation preparation custody from its actual
registered allocation and independently retained issuer control. Observed state,
raw IDs and caller-selected timestamps cannot construct it. Preparation shares
the original high-water and expiration owners and requires the original durable
checkpoint to acknowledge observations. This custody authorizes bounded
cancellation preparation, not VM admission, and does not acquire the execution
semaphore a second time.

A separately bounded ingress owner has no enrollment admission authority.
Historical negative publication retains its genuine prior terminal capability;
it cannot exchange that proof for a new active execution window. Required
preparation, checkpoint or publication failure preserves native causes and does
not report a successful completed handoff.

### Targeted VM disposal custody

Runtime session owner capabilities have private construction and read-only
metadata. Authorization requires the actual issued claim identity in addition
to matching session, generation, label and scope. Matching observations from a
different runtime grant no ingress, transfer or retirement authority. A handoff
invalidates every retained copy of the previous claim.

Required VM retirement disposes only the owned target session, including live or
blocked coroutines, scheduling eligibility, handoffs and scoped communication
and resource state. Unrelated sessions remain usable. Host metadata removal
follows the actual backend acknowledgment. The cooperative backend retains
stable coroutine IDs through its existing index rebuild; the threaded backend
validates and rebuilds its explicit ID index after removal.

Threaded worker execution is joined by the engine's synchronous worker scope
before exclusive disposal begins. Acknowledgment means no target job remains;
it does not require destroying the shared pool or other sessions. Naturally
terminal session status and epoch are preserved, and repeated disposal returns
its original compact acknowledgment. New close epoch advancement is checked
only for a genuinely active target.

Concrete backend failure remains in the standard native source chain. Required
closure cannot succeed by parsing or suppressing an unsupported error, merely
removing host metadata, or disposing an entire engine containing other sessions.
Historical compact summaries and diagnostic traces follow the dependency's
explicit archive policy. Shared global guard state is not claimed as exclusively
owned by the target session. VM disposal is local cleanup and does not establish
remote delivery, a signed terminal decision or protocol completion.

### Exact capability declaration evidence

A capability boundary declares its exact capability type in a parsed input or output. Semantic labels may specify `capability_type = Type`; labels and body text do not establish custody. Accessors return that exact type, authorizers retain that typed input or output, and proof issuers also declare their authoritative proof source. Runtime helpers with an actual held receiver may specify `receiver_type = OwnerType`; expansion checks the concrete receiver against that type. This receiver contract does not apply to free functions or replace authorization inputs in authorizers.

Constants, capability-like substrings, incidental body calls, phantom markers and associated projections do not satisfy the declaration. The declaration verifies API shape; private constructors and actual runtime ownership validation establish authority. Pure validators, pure execution-plan builders and observed projections are not capability issuers and carry no decorative capability-boundary declarations. Their domain tests and effect-placement rules remain required.

Configured capability types preserve their complete parsed path and generic
arguments. A boundary for `Owner<Capability>` cannot be satisfied by
`Owner<Other>` or an unparameterized owner with the same name.

A nominal capability declaration may omit borrow lifetime arguments from its
label. This permits an actual `Reservation<'a>` without erasing type or const
parameters; configured type and const arguments still require exact matches.

Canonical `AgentResult<T>` carries capability evidence only through its success
value `T`, using the same rule as `Result<T, E>`. An error type does not provide
custody. Actual production compilation and negative validator coverage enforce
this supported result-alias contract.

Qualified container names do not confer capability evidence by their final
identifier alone. A foreign wrapper named Result, Arc or AgentResult cannot
authorize a boundary by carrying an unrelated declaration parameter. Canonical
container paths preserve declared value evidence; actual capability custody
still requires opaque owner APIs and compiler-enforced ownership.

### Contact confirmation continuation

Required imported Contact metadata is read under a lease belonging to the actual
runtime. The retained acceptance capability transfers into a move-only signed
continuation only after the acceptance digest is acknowledged. Its confirmation
child shares the original operation's fixed deadline and physical observation
owner; retries cannot create a replacement interval.

A verified response retains the complete required import and runtime decision
lease through canonical contact publication and imported-status persistence.
Materialization consumes this evidence directly. Observed caches and descriptor
enrichment cannot select authoritative invitation state or delay the required
terminal publication on an unrelated reactive batch. App readiness remains an
independent authoritative observation contract.

In-process continuation does not establish restart authority. Durable restoration
requires the original parent and child checkpoints, signed payload and selected
profile binding; a retained digest alone cannot authorize a new wait.
