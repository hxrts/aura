# Toolkit Migration Status

## Notes

- `docs-semantic-drift` still uses explicit file exemptions for formula-heavy or
  known-stale docs while the reference corpus is cleaned up.
- `ci-lean-check-sorry` intentionally stays non-blocking because Aura still has
  a known `sorry`; the strict toolkit config is active for warning-only
  validation rather than as a required failure gate.
- `scripts/toolkit-shell.sh` now prefers the local `../toolkit` checkout when
  available and otherwise falls back to Aura's pinned consumer shell.

## Active Entry Points

- `fmt`, `fmt-check`, and `ci-format` now use `toolkit-fmt`.
- `clippy` and `ci-clippy` now use `toolkit-clippy`.
- `ci-crates-doc-links`, `ci-text-formatting`, and `ci-docs-semantic-drift`
  now use toolkit `xtask` checks.
- `check-arch` now runs the Aura-owned Rust entrypoint in `toolkit/xtask`
  instead of the deleted `scripts/check/arch.sh`.
- The ownership/runtime/testkit lanes moved to `toolkit/xtask` include:
  `ownership-category-declarations`, `service-surface-declarations`,
  `service-registry-ownership`, `ownership-annotation-ratchet`,
  `runtime-boundary-allowlist`, `runtime-shutdown-order`,
  `runtime-error-boundary`, `runtime-typed-lifecycle-bridge`,
  `ownership-workflow-tag-ratchet`, `observed-layer-boundaries`,
  `testing-exception-boundary`, `protocol-device-enrollment-contract`,
  `protocol-choreo-wiring`, `privacy-runtime-locality`,
  `privacy-legacy-sweep`, `harness-typed-semantic-errors`,
  `harness-typed-json-boundary`, `harness-authoritative-fact-boundary`,
  `harness-actor-vs-move-ownership`, and `browser-restart-boundary`.
- Additional Aura-specific shared-flow/runtime policy lanes now live in
  `toolkit/xtask` instead of shell:
  `protocol-device-id-legacy`,
  `runtime-bootstrap-guardrails`,
  `shared-flow-policy`,
  `shared-flow-metadata`,
  `shared-intent-flow`,
  `shared-raw-quarantine`,
  `shared-semantic-dedup`,
  `tui-observation-channel`,
  `tui-product-path`,
  `tui-selection-contract`, and
  `tui-semantic-snapshot`.
- Additional browser/harness/verification shell checks now also live in
  `toolkit/xtask`:
  `browser-cache-lifecycle`,
  `browser-cache-owner`,
  `browser-driver-contract-sync`,
  `browser-observation-recovery`,
  `harness-bridge-contract`,
  `harness-command-plane-boundary`,
  `harness-row-index-contract`,
  `harness-scenario-inventory`, and
  `verification-coverage`.
- The next harness policy tranche also moved from shell into `toolkit/xtask`:
  `harness-action-preconditions`,
  `harness-backend-contract`,
  `harness-boundary-policy`,
  `harness-conformance-gate`,
  `harness-export-override-policy`,
  `harness-focus-selection-contract`,
  `harness-matrix-inventory`,
  `harness-mode-allowlist`,
  `harness-observation-determinism`,
  `harness-observation-surface`,
  `harness-onboarding-contract`,
  `harness-onboarding-publication`,
  `harness-raw-backend-quarantine`,
  `harness-render-convergence`,
  `harness-revision-contract`,
  `harness-runtime-events-authoritative`,
  `harness-scenario-config-boundary`,
  `harness-semantic-primitive-contract`,
  `harness-wait-contract`, and
  `ownership-capability-audit`.
- The final retained shell-owned user-flow governance checks also moved into
  `toolkit/xtask`:
  `browser-restart-boundary`,
  `privacy-onion-quarantine`,
  `user-flow-guidance-sync`, and
  `user-flow-policy-guardrails`.
- The remaining active browser/harness wrapper checks that still sat on the
  shared-flow path also moved into `toolkit/xtask`:
  `browser-toolchain`,
  `browser-install`,
  `browser-driver-types`,
  `browser-observation-contract`,
  `harness-core-scenario-mechanics`,
  `harness-shared-scenario-contract`,
  `harness-scenario-legality`,
  `harness-scenario-shape-contract`,
  `harness-settings-surface-contract`,
  `harness-ui-parity-contract`,
  `harness-ui-state-evented`,
  `harness-trace-determinism`,
  `harness-recovery-contract`,
  `user-flow-coverage`, and
  `privacy-tuning-gate`.
- The old `scripts/check/testing-seed-uniqueness.sh` wrapper was removed as
  dead duplication because `check-arch --test-seeds` already enforced the same
  policy from `toolkit/xtask`.
- Repo-owned compiler-shape enforcement now includes
  `toolkit/lints/harness_boundaries`, consumed through `toolkit-dylint`.
- `ci-lean-check-sorry` now uses toolkit Lean style with the strict config, but
  it intentionally preserves the repo's current non-blocking warning behavior
  because Aura still has a known `sorry`.

## Validation Notes

- Confirmed green locally for the migrated entrypoints:
  `just check-arch --quick`,
  `just ci-format`,
  `just ci-clippy`,
  `just ci-crates-doc-links`,
  `just ci-text-formatting`,
  `just ci-docs-semantic-drift`,
  `just ci-ownership-categories`,
  `just ci-typed-errors`,
  `just ci-async-concurrency-envelope`,
  `just ci-runtime-instrumentation-schema`,
  `just ci-testkit-exception-boundary`,
  `just ci-choreo`,
  `just ci-service-registry-ownership`,
  `just ci-runtime-typed-lifecycle-bridge`,
  `just ci-workflow-ownership-tag-ratchet`,
  `just ci-harness-typed-json-boundary`,
  `just ci-harness-authoritative-fact-boundary`,
  `just ci-harness-ownership-policy`,
  `just ci-ownership-policy`,
  `just check-device-id-legacy`,
  `just audit-device-id-separation`,
  `just audit-runtime-device-id-separation`,
  `just check-bootstrap-guardrails`,
  `just ci-harness-tui-observation-channel`,
  `just ci-browser-driver-contract-sync`,
  `just harness-command-plane-boundary-check`,
  `just harness-scenario-inventory-check`,
  `just ci-harness-command-plane-boundary`,
  `just ci-harness-browser-observation-recovery`,
  `just harness-boundary-check`,
  `just ci-conformance-policy`,
  `just ci-harness-matrix-inventory`,
  `just ci-harness-runtime-events-authoritative`,
  `just ci-capability-model-audit`,
  `just ci-user-flow-policy`,
  `just ci-verification-coverage`,
  `just ci-shared-flow-policy`,
  and `just ci-lean-check-sorry`.
- Full repo validation is green after the shell deletions:
  `just ci-dry-run push` completed successfully on April 11, 2026 after
  clearing stale local build artifacts that had previously caused an
  infrastructure-only disk-space abort.
- `just toolkit-shadow` ran green 10 consecutive times against the unchanged
  working tree.
- Synthetic negative checks were validated against disposable temp repos for:
  strict Lean `sorry` rejection, text-formatting emoji rejection, and
  workspace-hygiene lonely-`mod.rs` rejection.
- Workflow audit: `.github/workflows/*.yml` no longer reference the deleted
  migrated shell scripts or any `legacy-*` recipes.
- The local toolkit wrapper now cleans stale Dylint caches when the toolkit
  nightly alias changes, avoiding incompatible-rustc cache poisoning during
  repeated runs.
- Draft upstream toolkit PR opened for the generic flake improvements:
  [`hxrts/toolkit#1`](https://github.com/hxrts/toolkit/pull/1).

## Remaining Gaps

- Docs semantic drift still relies on an explicit exemption set while the
  reference corpus is cleaned up.
- Aura still has an intentionally non-blocking Lean `sorry` lane because the
  repository contains an acknowledged incomplete Lean proof.
