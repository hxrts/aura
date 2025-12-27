# aura-effects

Stateless Layer 3 handler implementations for Aura.

## Optional Features

- `simulation` — Enables deterministic simulation handlers (fault injection, controlled time, and test-only effects). This feature is **off by default** to keep production builds minimal and to avoid simulation-only code paths.

## Behavior Notes

- Default builds use OS-backed randomness, time, storage, and networking handlers.
- Enabling `simulation` switches on deterministic, test-oriented handlers that should not be used in production.
