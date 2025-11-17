# Systemic Effect System Violations

This document lists known violations of the core architectural principle that side effects must be handled through the effect system. Direct calls to generate UUIDs or access system time undermine this pattern, making the system non-deterministic and harder to test.

Each item should be resolved by replacing the direct call with the appropriate effect from the `RandomEffects` or `TimeEffects` traits.

## Direct UUID Generation (`Uuid::new_v4`)

### `crates/aura-agent/src/handlers/ota.rs`
- [x] L272
### `crates/aura-agent/src/handlers/sessions.rs`
- [x] L408
### `crates/aura-agent/src/ota_orchestrator.rs`
- [x] L549
### `crates/aura-agent/src/runtime/context.rs`
- [x] L39
- [x] L77
- [x] L156
- [x] L157
- [x] L165
### `crates/aura-cli/src/handlers/ota.rs`
- [x] L82
### `crates/aura-effects/src/system/monitoring.rs`
- [x] L318
- [x] L384
- [x] L416
- [x] L452
- [x] L595
- [x] L920
### `crates/aura-effects/src/time.rs`
- [x] L141
- [x] L290
### `crates/aura-effects/src/transport/memory.rs`
- [x] L60
### `crates/aura-journal/src/journal.rs`
- [x] L325
### `crates/aura-journal/src/ledger/capability.rs`
- [x] L22
### `crates/aura-journal/src/ledger/intent.rs`
- [x] L27
### `crates/aura-journal/src/operations.rs`
- [x] L28
### `crates/aura-journal/src/semilattice/account_state.rs`
- [x] L317
### `crates/aura-mpst/src/context.rs`
- [x] L56
- [x] L62
- [x] L68
- [x] L79
### `crates/aura-protocol/src/effects/mod.rs`
- [x] L172
### `crates/aura-protocol/src/handlers/bridges/typed_bridge.rs`
- [x] L617
- [x] L645
- [x] L656
### `crates/aura-protocol/src/handlers/core/composite.rs`
- [x] L716
### `crates/aura-protocol/src/handlers/core/erased.rs`
- [x] L127
- [x] L153
### `crates/aura-protocol/src/handlers/context/context.rs`
- [x] L541
- [x] L548
- [x] L584
- [x] L607
- [x] L623
### `crates/aura-protocol/src/handlers/timeout_coordinator.rs`
- [x] L95
### `crates/aura-protocol/src/handlers/time_enhanced.rs`
- [x] L301
- [x] L354
- [x] L526
### `crates/aura-rendezvous/src/connection_manager.rs`
- [x] L65
### `crates/aura-sync/src/core/messages.rs`
- [x] L140
- [x] L315
- [x] L444
### `crates/aura-sync/src/protocols/ota.rs`
- [x] L190
- [x] L300
- [x] L322
- [x] L349
### `crates/aura-sync/src/protocols/snapshots.rs`
- [x] L71
- [x] L291
### `crates/aura-sync/src/services/maintenance.rs`
- [x] L86
### `crates/aura-verify/src/guardian.rs`
- [x] L170
### `crates/aura-verify/src/session.rs`
- [x] L178
- [x] L179
### `crates/aura-wot/src/tokens.rs`
- [x] L70

## Direct Time Access (`Instant::now`)

### `crates/aura-agent/src/optimizations/caching.rs`
- [x] L45
- [x] L50
### `crates/aura-agent/src/runtime/context.rs`
- [x] L85
- [x] L94
- [x] L384
### `crates/aura-agent/src/runtime/initialization.rs`
- [x] L44
### `crates/aura-agent/src/runtime/lifecycle.rs`
- [x] L113
- [x] L123
- [x] L169
- [x] L350
### `crates/aura-agent/src/runtime/reliability.rs`
- [x] L84
- [x] L107
- [x] L496
### `crates/aura-core/src/effects/reliability.rs`
- [x] L333
- [x] L429
- [x] L516
- [x] L526
- [x] L533
### `crates/aura-effects/src/system/metrics.rs`
- [x] L502
### `crates/aura-effects/src/system/monitoring.rs`
- [x] L276
- [x] L492
### `crates/aura-effects/src/transport/utils.rs`
- [x] L198
- [x] L205
- [x] L210
- [x] L211
### `crates/aura-protocol/src/guards/deltas.rs`
- [x] L29
### `crates/aura-protocol/src/guards/evaluation.rs`
- [x] L61
- [x] L70
- [x] L81
- [x] L164
### `crates/aura-protocol/src/guards/execution.rs`
- [x] L40
- [x] L47
- [x] L67
- [x] L76
- [x] L177
- [x] L210
### `crates/aura-protocol/src/guards/journal_coupler.rs`
- [x] L160
- [x] L194
- [x] L259
### `crates/aura-protocol/src/guards/send_guard.rs`
- [x] L108
- [x] L121
- [x] L154
### `crates/aura-protocol/src/handlers/time_enhanced.rs`
- [x] L124
- [x] L281
- [x] L307
- [x] L355
- [x] L493
### `crates/aura-protocol/src/handlers/transport_coordinator.rs`
- [x] L209
- [x] L227
- [x] L285
### `crates/aura-quint-api/src/runner.rs`
- [x] L272
- [x] L424
### `crates/aura-rendezvous/src/connection_manager.rs`
- [x] L500
- [x] L513
### `crates/aura-sync/src/core/metrics.rs`
- [x] L317
### `crates/aura-sync/src/core/session.rs`
- [x] L267
- [x] L277
- [x] L542
### `crates/aura-sync/src/infrastructure/connections.rs`
- [x] L119
- [x] L146
- [x] L154
- [x] L212
### `crates/aura-sync/src/infrastructure/peers.rs`
- [x] L141
- [x] L159
- [x] L275
### `crates/aura-sync/src/protocols/journal.rs`
- [x] L210
### `crates/aura-sync/src/services/maintenance.rs`
- [x] L452
### `crates/aura-sync/src/services/sync.rs`
- [x] L254
