# Policy

This directory owns Aura-specific policy configuration and repo-local policy code.

- Generic reusable Rust and Lean checks belong in `../toolkit`.
- Aura-specific architecture, ownership, and boundary rules stay here.
- Toolkit consumption is configured through `policy/toolkit.toml`.
- Future Aura-local policy code should live under `policy/checks/`, `policy/lints/`,
  `policy/fixtures/`, and `policy/xtask/`.
- Repo-local shadow entrypoints can invoke `cargo run --manifest-path policy/xtask/Cargo.toml -- check <name>`.
