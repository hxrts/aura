# Reactive Module Boundary

This directory contains Aura's reactive pipeline (FRP primitives + scheduler).

## Public API (stable)
- `Dynamic<T>` (FRP primitive)
- `ReactivePipeline` (facts → scheduler → view updates)
- `ReactiveScheduler` + `SchedulerConfig`
- `ReactiveView`, `ViewReduction`, `ViewAdapter`, `ViewUpdate`
- Signal views: `ChatSignalView`, `InvitationsSignalView`, `ContactsSignalView`, `RecoverySignalView`, `HomeSignalView`
- Domain delta types re-exported by `reactive/mod.rs`

## Internal API (crate-only)
All implementation modules (`frp`, `scheduler`, `pipeline`, `app_signal_views`, `reductions`, `state`)
are `pub(crate)` and should not be depended on outside `aura-agent`.

If a new API surface is needed, re-export it explicitly from `reactive/mod.rs`
and update this README.
