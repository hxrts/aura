# Ownership Model

This document defines Aura's repo-wide ownership model for authority,
mutation, async lifecycle, and terminal failure behavior.

It complements:

- [System Architecture](001_system_architecture.md) for the high-level system
  view
- [Effect System](103_effect_system.md) for effect boundaries
- [Runtime](104_runtime.md) for lifecycle and supervision
- [Project Structure](999_project_structure.md) for layer placement

## Overview

Aura uses four ownership categories:

- `Pure`
- `MoveOwned`
- `ActorOwned`
- `Observed`

Every parity-critical subsystem, operation, and mutation/publication surface
must fit one of these categories. Bugs of the form "multiple layers own the
same truth" are treated as architectural violations.

## Categories

### `Pure`

`Pure` code is deterministic and effect-free.

Use `Pure` for:

- reducers
- validators
- state machines
- fact interpretation
- typed contracts and value types

`Pure` code may not:

- own long-lived mutable async state
- publish semantic lifecycle or readiness by itself
- rely on ambient authority

### `MoveOwned`

`MoveOwned` code represents exclusive authority through consumed values.

Use `MoveOwned` for:

- operation handles
- owner tokens
- delegation records
- session or endpoint handoff objects
- stale-owner invalidation boundaries

Rules:

- ownership transfer must consume a handoff object or owner token
- stale holders must become invalid by construction
- direct owner-field rewrites are forbidden where a transfer object is required

### `ActorOwned`

`ActorOwned` code owns long-lived mutable async state under one live task.

Use `ActorOwned` for:

- runtime services
- supervisors
- maintenance loops
- readiness coordinators
- lifecycle coordinators
- command ingress loops

Rules:

- there is exactly one live owner for the mutable state domain
- mutation happens through typed ingress, not shared mutable access
- long-lived background work must be supervised
- owner death must lead to explicit terminal state, failure, or shutdown

### `Observed`

`Observed` code reads and presents authoritative state but does not own it.

Use `Observed` for:

- projections
- UI rendering
- harness reads
- diagnostics
- reporting

Rules:

- `Observed` code may submit typed commands to owner surfaces
- `Observed` code may not author semantic lifecycle or readiness truth
- `Observed` code may not repair ownership mistakes by mutating product state

## Capability-Gated Authority

Aura already has a capability system. The ownership model builds on that rather
than introducing an unrelated permission scheme.

Parity-critical mutation and publication should be capability-gated:

- semantic lifecycle publication requires an appropriate capability
- readiness publication requires a coordinator-owned capability
- ownership transfer requires a transfer capability or sanctioned handoff token
- actor ingress that mutates owned state requires the actor's command boundary

The goal is to make incorrect authority structurally hard to express. Code
should not be able to publish semantic truth merely because it can call a
helper.

## Usage Examples

### When To Use `Pure`

Use `Pure` when the code is interpreting values rather than owning authority or
async lifecycle:

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

This stays `Pure` because it:

- consumes values
- returns a value
- owns no long-lived state
- does not publish lifecycle/readiness directly

### When To Use `MoveOwned`

Use `MoveOwned` when stale access must become invalid after handoff:

```rust
use aura_core::{
    issue_owner_token, OwnershipTransferCapability,
};

let capability = OwnershipTransferCapability::new("ownership:transfer");
let token = issue_owner_token(&capability, "invite-op-7", "channel:alpha");
let transfer = token.handoff("invite-coordinator");
```

This is `MoveOwned` because the original `token` is consumed by `handoff`.
Trying to keep acting through the old owner is a compile-time error.

### When To Use Capability Tokens

Use capability wrappers whenever parity-critical code needs authority to author
semantic truth:

```rust
use aura_core::{
    AuthorizedLifecyclePublication, LifecyclePublicationCapability,
    OperationLifecycle,
};

let capability = LifecyclePublicationCapability::new("semantic:lifecycle");
let update = AuthorizedLifecyclePublication::authorize(
    &capability,
    OperationLifecycle::<&'static str, (), &'static str>::progress("waiting"),
);
```

The important property is not the wrapper itself. The important property is
that publication requires a capability-shaped input, so random helper code
cannot publish lifecycle by accident.

### When To Use `ActorOwned`

Use `ActorOwned` when one live task must own mutable async state and terminal
responsibility:

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

This is `ActorOwned` because:

- there is one live owner of `pending`
- mutation happens through typed ingress
- owner drop is a lifecycle event that must be surfaced explicitly

## Selection Heuristics

When choosing between categories:

- choose `Pure` first if the logic can be expressed as value-in/value-out
- choose `MoveOwned` when the hard problem is exclusive authority or stale
  holder invalidation
- choose `ActorOwned` when the hard problem is long-lived mutable async state
  under one live owner
- choose `Observed` only for read-only presentation or diagnostics

Anti-patterns:

- a shell callback publishing semantic success: should be `Observed`, not an
  owner
- shared mutable `Arc<Mutex<_>>` state spread across tasks: should usually be
  `ActorOwned`
- rewriting an owner field in place after delegation: should usually be
  `MoveOwned`
- reducers that call time/network/storage directly: no longer `Pure`

## Terminality

Every parity-critical operation must have typed terminal behavior.

Direct boundaries use typed results:

```rust
Result<T, E>
```

Long-running operations use typed lifecycle:

- `Submitted`
- zero or more typed intermediate phases
- `Succeeded`
- `Failed(E)`
- `Cancelled`

Rules:

- every submitted operation must reach a terminal state
- terminal states may not regress
- owner drop must publish failure or cancellation explicitly

## Layer Guidance

### Layer 1: `aura-core`

Defines the shared ownership vocabulary:

- ownership/category primitives
- typed lifecycle and terminality helpers
- typed ownership/capability boundaries

### Layer 2: domain crates

Default to `Pure`.

Use `MoveOwned` only when transfer semantics are part of the domain itself.
Domain crates should not silently grow runtime-style async ownership.

### Layer 3: implementation crates

Default to stateless handlers. Avoid long-lived mutable ownership except for
narrow adapter internals.

### Layer 4: orchestration

Use:

- `MoveOwned` for delegation, session ownership, and handoff
- `ActorOwned` for long-lived orchestration coordinators only

### Layer 5: feature crates

Feature workflows should have single-owner semantic lifecycle. Wrappers,
views, and shells must not co-author stronger semantics than canonical
workflows.

### Layer 6: runtime

Primary `ActorOwned` layer. Runtime services, caches, maintenance loops, and
supervisors should be actor-owned. Ownership transfer still uses `MoveOwned`
surfaces rather than mailbox identity alone.

### Layer 7: interface

Primarily `Observed`. Frontends may provide command ingress mechanics but do
not own parity-critical semantic truth.

### Layer 8: testing

Testing infrastructure may simulate actors and capabilities, but parity-critical
lanes must observe and submit through the same ownership boundaries as
production code.

## Enforcement

The ownership model is enforced in layers:

1. types and private constructors
2. capability-gated mutation/publication APIs
3. thin policy checks in `scripts/check/*.sh`
4. `just ci-*` recipes that follow existing project CI conventions
5. compile-fail tests for private/capability boundaries
6. invariant and concurrency tests for owner drop, stale-handle invalidation,
   and terminality

Scripts alone are not sufficient. The API must make the wrong pattern hard or
impossible first.

## Review Checklist

When adding or modifying a parity-critical path, reviewers should ask:

- what category is this module or subsystem: `Pure`, `MoveOwned`,
  `ActorOwned`, or `Observed`?
- who is the single live owner of mutable async state?
- how is authority transferred?
- what capability authorizes mutation/publication?
- what is the typed terminal success/failure contract?
- what legacy bypasses should be deleted rather than preserved?

If these answers are unclear, the design is not complete enough.
