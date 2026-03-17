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

### `Observed`

`Observed` code reads and presents authoritative state but does not own it. Use it for projections, UI rendering, harness reads, diagnostics, and reporting.

`Observed` code may submit typed commands to owner surfaces. It may not author semantic lifecycle or readiness truth. It may not repair ownership mistakes by mutating product state.

## Capability-Gated Authority

The ownership model builds on Aura's existing capability system. Parity-critical mutation and publication should be capability-gated.

Semantic lifecycle publication requires an appropriate capability. Readiness publication requires a coordinator-owned capability. Ownership transfer requires a transfer capability or sanctioned handoff token. Actor ingress that mutates owned state requires the actor's command boundary.

The goal is to make incorrect authority structurally hard to express. Code should not be able to publish semantic truth merely because it can call a helper.

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

Typed ownership capabilities from the same wrapper family can also be issued
onto Aura's existing Biscuit path without first down-converting them to raw
`CapabilityKey` values via `ownership_capability_token_request_for(...)`.
Lower layers should not expose parallel raw ownership-capability request
helpers once a typed wrapper family exists.

### When To Use Capability Tokens

Use capability wrappers whenever parity-critical code needs authority to author semantic truth.

```rust
use aura_core::{
    AuthorizedLifecyclePublication, AuthorizedReadinessPublication,
    LifecyclePublicationCapability, OperationLifecycle,
    ReadinessPublicationCapability,
};

let capability = LifecyclePublicationCapability::new("semantic:lifecycle");
let update = AuthorizedLifecyclePublication::authorize(
    &capability,
    OperationLifecycle::<&'static str, (), &'static str>::progress("waiting"),
);
```

Publication requires a capability-shaped input. Random helper code cannot publish lifecycle by accident.

The same rule applies to readiness and actor-ingress mutation. Higher layers
should prefer `AuthorizedReadinessPublication<T>` and
`AuthorizedActorIngressMutation<T>` over raw capability arguments when they
need to move parity-critical authority across API boundaries.

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

## Terminality

Every parity-critical operation must have typed terminal behavior. Direct boundaries use `Result<T, E>`. Long-running operations use typed lifecycle phases: `Submitted`, zero or more intermediate phases, then `Succeeded`, `Failed(E)`, or `Cancelled`.

Every submitted operation must reach a terminal state. Terminal states may not regress. Owner drop must publish failure or cancellation explicitly.

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

### Layer 1

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-core` | `Pure`, `MoveOwned` | none | operation handles, owner tokens, capability wrappers, time-budget records | capability issuance, lifecycle helpers, timeout-policy constructors | typed-error migration incomplete in cross-crate adapter traits |

### Layer 2

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-journal` | `Pure` | none | append batches, reducer input records, fact-key wrappers | journal append and reduction entrypoints | stringly error boundary sites |
| `aura-authorization` | `Pure`, `MoveOwned` | none | attenuation chains, capability frontier transfer records | biscuit validation and attenuation issuance | capability lint allowlist entries |
| `aura-signature` | `Pure` | none | signing session inputs, proof bundles | signature issuance and proof publication | typed-error cleanup pending |
| `aura-store` | `Pure` | none | batch descriptors, keyspace transfer records | storage write admission | timeout-policy allowlist around store waits |
| `aura-transport` | `Pure`, `MoveOwned` | none | connection descriptors, receipt ownership records | receipt issuance, transport send boundaries | typed-error and timeout allowlists |
| `aura-mpst` | `Pure`, `MoveOwned` | none | session endpoints, protocol continuations | endpoint progression | older ownership wrappers still in use |
| `aura-macros` | `Pure` | none | none | none | none |
| `aura-maintenance` | `Pure` | none | maintenance command descriptors, rollout records | maintenance-plan issuance | timeout-policy rollout incomplete |

### Layer 3

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-effects` | `Pure` | none | IO handles, adapter request records | effect entrypoints consuming capability commands | typed-error and timeout allowlists |
| `aura-composition` | `Pure` | none | runtime bundle assembly records | composition-time adapter assembly | capability-boundary allowlists |

### Layer 4

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-protocol` | `MoveOwned`, `ActorOwned` | protocol/session coordinators | session handles, continuation tokens | protocol progress publication | actor-lifecycle exemptions |
| `aura-guards` | `Pure`, `MoveOwned` | none | charged budget records, guard result tokens | capability and budget charge publication | typed-error cleanup pending |
| `aura-consensus` | `MoveOwned`, `ActorOwned` | round coordinators, vote collection | proposal tokens, quorum certificates | vote admission, round-finalization | actor-owned lifecycle inventory open |
| `aura-amp` | `MoveOwned`, `ActorOwned` | channel/state coordinators | channel bindings, bootstrap tokens | channel bootstrap, membership progression | authority mismatches and timeout debt |
| `aura-anti-entropy` | `MoveOwned`, `ActorOwned` | reconciliation supervisors | reconciliation handles, checkpoint cursors | reconciliation progress publication | actor/task lifecycle cleanup pending |

#### Layer 4 audit findings

The current Layer 4 ownership audit found the following concrete shared-state
surfaces that need explicit classification and follow-up refactors:

- `aura-protocol`
  - `handlers/timeout_coordinator.rs` is still a thin wrapper today, but any
    future global timeout/context registry must be introduced as an explicit
    `ActorOwned` coordinator rather than as a shared lock registry.
  - `handlers/transport_coordinator.rs` no longer spreads its connection
    registry across clones via `Arc<RwLock<_>>`; the remaining follow-up is to
    decide whether it should stay as a single-owner coordinator object or grow
    into a dedicated owner task with command ingress.
  - `handlers/context/agent.rs` clones and replaces session maps by hand. This
    is a `MoveOwned` candidate: session ownership and handoff should be
    expressed as consumed transfer values rather than ad hoc map rewrites.
- `aura-consensus`
  - `frost.rs`, `protocol/logic.rs`, and `witness.rs` no longer spread active
    instances and witness state through `Arc<RwLock<HashMap<...>>>` clones; the
    remaining work is to decide which of these coordinator-owned state holders
    should stay as single-owner coordinator objects and which need explicit
    actor ingress once cross-task ownership appears.
  - `evidence.rs` keeps mutable evidence trackers in plain `HashMap`s. This is
    acceptable only while single-owner and local; any async sharing should move
    behind an actor boundary.
- `aura-anti-entropy`
  - `anti_entropy.rs` and `broadcast.rs` no longer spread oplogs, peer sets,
    announcement queues, and rate limits across `Arc<RwLock<_>>` registries;
    they now keep coordinator-owned state objects. `persistent.rs` and any
    future background reconciliation services still need the same treatment if
    they become long-lived async owners.
- `aura-amp`
  - the audit did not find direct owner-field rewrites, but AMP still carries
    orchestration state in consensus/bootstrap helpers that should be reviewed
    alongside the channel bootstrap and membership coordinator work.

The audit did not find remaining direct move-owned field rewrites in Layer 4,
but it did confirm that several orchestration crates still rely on shared
mutable registries where a single-owner actor or a consumed handoff surface is
the correct model.

### Layer 5

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-authentication` | `MoveOwned`, `ActorOwned` | ceremony coordinators | challenge/response handles | authentication result publication | timeout-policy exemptions |
| `aura-chat` | `Pure`, `MoveOwned` | none | send handles, receipt progression records | delivery-state publication | typed-error cleanup pending |
| `aura-invitation` | `MoveOwned`, `ActorOwned` | invitation creation/acceptance coordinators | invitation records, accept/import handles | invitation lifecycle publication | accept/bootstrap authority debt |
| `aura-recovery` | `MoveOwned`, `ActorOwned` | recovery ceremonies, guardian coordination | recovery grants, ceremony handles | recovery grant and ceremony publication | actor/task lifecycle inventory open |
| `aura-relational` | `Pure`, `MoveOwned` | none | relational context grants | context mutation admission | typed-error exemptions |
| `aura-rendezvous` | `MoveOwned`, `ActorOwned` | rendezvous sessions | rendezvous tickets, session handles | rendezvous match publication | actor/task lifecycle exemptions |
| `aura-social` | `Pure`, `MoveOwned` | none | contact-link and neighborhood transfer records | social-link mutation admission | typed-error cleanup |
| `aura-sync` | `MoveOwned`, `ActorOwned` | sync coordinators, backfill workers | sync handles, checkpoint cursors | sync progress publication | timeout/backoff rollout pending |

### Layer 6

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-agent` | `ActorOwned`, `MoveOwned`, `Observed` | maintenance, invitation ingress, LAN discovery, journal/cache services | service commands, operation handles, runtime bridge tokens | journal/fact publication, capability-bearing runtime actions | legacy detached background work, timeout wrappers |
| `aura-simulator` | `ActorOwned`, `Observed` | simulation coordinators | simulation handles | simulated fact/publication boundaries | deeper module inventory pending |
| `aura-app` | `Pure`, `MoveOwned` | narrow background coordinators only | semantic operation handles, owner tokens, typed timeout budgets | authoritative lifecycle/readiness publication | timeout-policy and typed-error allowlist debt |

### Layer 7

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-terminal` | `Observed`, narrow `ActorOwned` ingress | TUI ingress loop only | harness command receipts, operation-instance records | none beyond capability-bearing handoff into `aura-app` | legacy lifecycle ownership in invitation/channel flows |
| `aura-ui` | `Observed` | none | none | none | module inventory pending |
| `aura-web` | `Observed`, narrow `ActorOwned` bridge | browser harness bridge only | browser semantic command requests/receipts | none beyond sanctioned bridge/handoff points | timeout-policy and typed-error allowlist debt |

### Layer 8

| Crate | Categories | Actor-owned domains | Move-owned surfaces | Capability-gated points | Known debt |
|-------|-----------|---------------------|---------------------|------------------------|------------|
| `aura-testkit` | `Observed`, test-only `ActorOwned` helpers | mock services and supervised test coordinators | mocked session/operation handles | mocked publication points mirror production | deeper module inventory pending |
| `aura-quint` | `Pure`, `Observed` | none | none | none | none |
| `aura-harness` | `Observed`, orchestration-local `ActorOwned` | backend supervisors, scenario executors | semantic command handles, wait tokens | none (harness-local diagnostics only) | timeout-policy debt, legacy non-shared flows |

Each crate migration must refine its own `ARCHITECTURE.md` so that parity-critical modules are classified, actor-owned domains have explicit owners, move-owned surfaces identify consumed transfer APIs, and capability-gated points are named. Legacy violations should be removed rather than indefinitely allowlisted.

## Enforcement

The ownership model is enforced in layers.

Types and private constructors provide the first line of defense. Capability-gated mutation and publication APIs form the second layer. Thin policy checks in `scripts/check/*.sh` and `just ci-*` recipes provide CI enforcement. Compile-fail tests validate private/capability boundaries. Invariant and concurrency tests verify owner drop, stale-handle invalidation, and terminality.

Scripts alone are not sufficient. The API must make the wrong pattern hard or impossible first.

## Review Checklist

When adding or modifying a parity-critical path, ask these questions:

- What category is this module or subsystem?
- Who is the single live owner of mutable async state?
- How is authority transferred?
- What capability authorizes mutation or publication?
- What is the typed terminal success/failure contract?
- What legacy bypasses should be deleted rather than preserved?

If these answers are unclear, the design is not complete enough.
