# Environment Configuration Guide

This guide is the working registry for Aura environment variables.

Use three buckets:
- `product runtime`: user-facing runtime configuration that affects production shells or handlers
- `harness / tooling`: local automation, browser harness, CI, or workflow bring-up knobs
- `test-only`: fixture generation, compatibility baselines, compile-fail helpers, or artifact capture used only in tests

Do not add new env reads ad hoc. Add the variable to the appropriate bucket first, then expose it through a small typed helper near the owning runtime boundary.

## Product Runtime

| Variable | Owner | Purpose |
|----------|-------|---------|
| `AURA_PATH` | `aura-agent::core::config` | Base root used to resolve the default Aura storage directory |
| `AURA_BOOTSTRAP_BROKER_BIND` | `aura-terminal::env` | Override bootstrap-broker bind address for mixed native/browser startup |
| `AURA_BOOTSTRAP_BROKER_URL` | `aura-terminal::env` | Override externally reachable bootstrap-broker base URL |
| `AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND` | `aura-terminal::env` | Explicitly allow a bootstrap broker bind address that is visible off loopback |
| `AURA_BOOTSTRAP_BROKER_AUTH_TOKEN` | `aura-terminal::env` | Bearer token required for bootstrap-broker HTTP endpoints |
| `AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN` | `aura-terminal::env` | Unguessable one-time token used to drain bootstrap-broker invitations |
| `AURA_CLIPBOARD_MODE` | `aura-terminal::env` | Select clipboard behavior for terminal code display (`system`, `file_only`, `disabled`) |
| `AURA_CLIPBOARD_FILE` | `aura-terminal::env` | Capture clipboard writes to a file for constrained or automated environments |
| `AURA_DEMO_DEVICE_ID` | `aura-terminal::env` | Enable demo-only neighborhood assist behavior when a demo device id is staged |
| `AURA_TCP_LISTEN_ADDR` | `aura-effects::transport::env` | Override the stateless TCP receive bind address |
| `AURA_TUI_ALLOW_STDIO` | `aura-terminal::env` | Disable fullscreen stdio redirection for debugging |
| `AURA_TUI_LOG_PATH` | `aura-terminal::env` | Override the storage key/path used for persisted TUI logs |

## Harness / Tooling

| Variable family | Owner | Purpose |
|-----------------|-------|---------|
| `AURA_HARNESS_*` | `aura-harness`, `aura-app::workflows::runtime`, `aura-agent::runtime_bridge`, `aura-ui::app::shell::modal_submit` | Harness orchestration, convergence, browser bootstrap, and render-stability controls |
| `AURA_WEB_APP_URL` | `aura-harness` | Fallback browser app URL for harness/browser bring-up |
| `AURA_ALLOW_FLOW_COVERAGE_SKIP`, `AURA_FLOW_COVERAGE_*` | `aura-harness::governance` | Governance/coverage policy knobs |
| `GITHUB_*`, `CI` | harness/tooling only | CI-aware defaults for harness governance and parity rotation |

Harness env reads may stay close to the owning harness boundary, but they should still be routed through small typed helpers when reused in multiple places.

## Test-only

Examples:
- `AURA_PROTOCOL_COMPAT_*`
- `AURA_CONSENSUS_ITF_*`
- `AURA_CONFORMANCE_*`
- `AURA_PROPERTY_MONITOR_*`
- `AURA_TELLTALE_*`
- compile-fail helpers that only inspect Cargo-provided environment variables

These should not be mixed into product-runtime or harness-runtime registries. Keep them local to the owning test module unless they become shared fixture infrastructure.
