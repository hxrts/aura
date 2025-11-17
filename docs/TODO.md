# Systemic Effect System Violations

This document lists known violations of the core architectural principle that side effects must be handled through the effect system. Direct calls to generate UUIDs or access system time undermine this pattern, making the system non-deterministic and harder to test.

Each item should be resolved by replacing the direct call with the appropriate effect from the `RandomEffects` or `TimeEffects` traits.

## Direct UUID Generation (`Uuid::new_v4`)

### `crates/aura-agent/src/handlers/ota.rs`
- [ ] L272
### `crates/aura-agent/src/handlers/sessions.rs`
- [ ] L408
### `crates/aura-agent/src/ota_orchestrator.rs`
- [ ] L549
### `crates/aura-agent/src/runtime/context.rs`
- [ ] L39
- [ ] L77
- [ ] L156
- [ ] L157
- [ ] L165
### `crates/aura-cli/src/handlers/ota.rs`
- [ ] L82
### `crates/aura-effects/src/system/monitoring.rs`
- [ ] L318
- [ ] L384
- [ ] L416
- [ ] L452
- [ ] L595
- [ ] L920
### `crates/aura-effects/src/time.rs`
- [ ] L141
- [ ] L290
### `crates/aura-effects/src/transport/memory.rs`
- [ ] L60
### `crates/aura-journal/src/journal.rs`
- [ ] L325
### `crates/aura-journal/src/ledger/capability.rs`
- [ ] L22
### `crates/aura-journal/src/ledger/intent.rs`
- [ ] L27
### `crates/aura-journal/src/operations.rs`
- [ ] L28
### `crates/aura-journal/src/semilattice/account_state.rs`
- [ ] L317
### `crates/aura-mpst/src/context.rs`
- [ ] L56
- [ ] L62
- [ ] L68
- [ ] L79
### `crates/aura-protocol/src/effects/mod.rs`
- [ ] L172
### `crates/aura-protocol/src/handlers/bridges/typed_bridge.rs`
- [ ] L617
- [ ] L645
- [ ] L656
### `crates/aura-protocol/src/handlers/core/composite.rs`
- [ ] L716
### `crates/aura-protocol/src/handlers/core/erased.rs`
- [ ] L127
- [ ] L153
### `crates/aura-protocol/src/handlers/context/context.rs`
- [ ] L541
- [ ] L548
- [ ] L584
- [ ] L607
- [ ] L623
### `crates/aura-protocol/src/handlers/timeout_coordinator.rs`
- [ ] L95
### `crates/aura-protocol/src/handlers/time_enhanced.rs`
- [ ] L301
- [ ] L354
- [ ] L526
### `crates/aura-rendezvous/src/connection_manager.rs`
- [ ] L65
### `crates/aura-sync/src/core/messages.rs`
- [ ] L140
- [ ] L315
- [ ] L444
### `crates/aura-sync/src/protocols/ota.rs`
- [ ] L190
- [ ] L300
- [ ] L322
- [ ] L349
### `crates/aura-sync/src/protocols/snapshots.rs`
- [ ] L71
- [ ] L291
### `crates/aura-sync/src/services/maintenance.rs`
- [ ] L86
### `crates/aura-verify/src/guardian.rs`
- [ ] L170
### `crates/aura-verify/src/session.rs`
- [ ] L178
- [ ] L179
### `crates/aura-wot/src/tokens.rs`
- [ ] L70

## Direct Time Access (`Instant::now`)

### `crates/aura-agent/src/optimizations/caching.rs`
- [ ] L45
- [ ] L50
### `crates/aura-agent/src/runtime/context.rs`
- [ ] L85
- [ ] L94
- [ ] L384
### `crates/aura-agent/src/runtime/initialization.rs`
- [ ] L44
### `crates/aura-agent/src/runtime/lifecycle.rs`
- [ ] L113
- [ ] L123
- [ ] L169
- [ ] L350
### `crates/aura-agent/src/runtime/reliability.rs`
- [ ] L84
- [ ] L107
- [ ] L496
### `crates/aura-core/src/effects/reliability.rs`
- [ ] L333
- [ ] L429
- [ ] L516
- [ ] L526
- [ ] L533
### `crates/aura-effects/src/system/metrics.rs`
- [ ] L502
### `crates/aura-effects/src/system/monitoring.rs`
- [ ] L276
- [ ] L492
### `crates/aura-effects/src/transport/utils.rs`
- [ ] L198
- [ ] L205
- [ ] L210
- [ ] L211
### `crates/aura-protocol/src/guards/deltas.rs`
- [ ] L29
### `crates/aura-protocol/src/guards/evaluation.rs`
- [ ] L61
- [ ] L70
- [ ] L81
- [ ] L164
### `crates/aura-protocol/src/guards/execution.rs`
- [ ] L40
- [ ] L47
- [ ] L67
- [ ] L76
- [ ] L177
- [ ] L210
### `crates/aura-protocol/src/guards/journal_coupler.rs`
- [ ] L160
- [ ] L194
- [ ] L259
### `crates/aura-protocol/src/guards/send_guard.rs`
- [ ] L108
- [ ] L121
- [ ] L154
### `crates/aura-protocol/src/handlers/time_enhanced.rs`
- [ ] L124
- [ ] L281
- [ ] L307
- [ ] L355
- [ ] L493
### `crates/aura-protocol/src/handlers/transport_coordinator.rs`
- [ ] L209
- [ ] L227
- [ ] L285
### `crates/aura-quint-api/src/runner.rs`
- [ ] L272
- [ ] L424
### `crates/aura-rendezvous/src/connection_manager.rs`
- [ ] L500
- [ ] L513
### `crates/aura-sync/src/core/metrics.rs`
- [ ] L317
### `crates/aura-sync/src/core/session.rs`
- [ ] L267
- [ ] L277
- [ ] L542
### `crates/aura-sync/src/infrastructure/connections.rs`
- [ ] L119
- [ ] L146
- [ ] L154
- [ ] L212
### `crates/aura-sync/src/infrastructure/peers.rs`
- [ ] L141
- [ ] L159
- [ ] L275
### `crates/aura-sync/src/protocols/journal.rs`
- [ ] L210
### `crates/aura-sync/src/services/maintenance.rs`
- [ ] L452
### `crates/aura-sync/src/services/sync.rs`
- [ ] L254
