# Aura Protocol (Layer 4) - Architecture and Invariants

## Purpose
Coordinate multi-party protocols and guard-chain enforcement. This crate provides
orchestration glue, not single-party effect implementations.

## Inputs
- Effect trait implementations assembled by higher layers (agent/simulator).
- Choreographic annotations and session types.
- Journal and authorization facts from domain crates.

## Outputs
- Guarded transport operations and protocol outcomes.
- Orchestrated consensus and anti-entropy flows.

## Invariants
- No production effect implementations live in Layer 4.
- Guard chain is enforced on every send.
- Journal facts and budgets are coupled atomically before transport.

## Ownership Model

- `aura-protocol` uses `MoveOwned` for delegation, session transfer, and other
  exclusive orchestration boundaries.
- It should use `ActorOwned` state only for justified long-lived coordinators,
  not as the default for protocol ownership.
- Capability-gated publication and mutation must remain explicit in orchestration
  paths.
- Async orchestration flows must reach typed terminal outcomes rather than
  relying on implicit success or silent timeout.
- `Observed` layers consume protocol outputs downstream and must not co-author
  protocol truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| protocol/session handlers and core builder/config modules | `MoveOwned` | Session transfer, delegation, and typed orchestration boundaries. |
| long-lived coordinators such as `transport_coordinator` and peer-connection retry actors | `ActorOwned` | Justified orchestration coordinators only; not the default model for protocol logic. |
| guard-chain and effect integration surfaces | capability-gated orchestration | Capability, flow, and journal coupling remain explicit on send paths. |
| observed-only surfaces | none local | Observation belongs in higher layers consuming protocol outputs. |

### Current Debt

- `transport_coordinator` now uses structured terminal transport failures, but
  other protocol-local error surfaces still need the same typed treatment
  before the crate pass is complete.
- coordinator ownership must remain explicit and single-owner; no drift back to
  shared lock registries or shell-driven lifecycle.

### Verification Hooks

- `cargo check -p aura-protocol`
- `cargo test -p aura-protocol --test transport_coordinator -- --nocapture`
- `just check-arch`

### Detailed Specifications

### InvariantProtocolGuardMediation
Protocol sends must be mediated by the guard chain with budget and journal coupling before transport.

Enforcement locus:
- src handlers and sessions integrate guard decisions into send paths.
- Protocol modules avoid direct production effect implementations.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-protocol and just check-arch

Contract alignment:
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) defines charge-before-send behavior.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines fact-backed send requirements.
## Boundaries
- No runtime composition or lifecycle management (Layer 6 responsibility).
- No application-specific protocol logic (Layer 5 responsibility).

## Core + Orchestrator Rule
- Any new protocol logic should be split into pure core and effectful orchestrator modules.
