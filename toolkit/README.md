# Policy

This directory owns Aura-specific policy configuration and repo-local policy code.

- Generic reusable Rust and Lean checks belong in `../toolkit`.
- Aura-specific architecture, ownership, and boundary rules stay here.
- Toolkit consumption is configured through `toolkit/toolkit.toml`.
- Future Aura-local policy code should live under `policy/checks/`, `toolkit/lints/`,
  `policy/fixtures/`, and `toolkit/xtask/`.
- Repo-local shadow entrypoints can invoke `cargo run --manifest-path toolkit/xtask/Cargo.toml -- check <name>`.

Ownership coverage explicitly runs app, agent and signals compile-fail guards plus the native forced-process lock regression. Required guard suites fail if Cargo cannot execute. The signals lane passes `--features signals`; disabled feature coverage is absent, never an empty green test. Native harnesses share aura-testkit's one workspace-derived descriptor lock and never remove/recreate its inode.

Required ownership coverage checks three separate forms of evidence: Rust test attributes without ignore annotations, exact names published by the selected harness, and successful execution of every required test. The signals harness is selected with `--features signals`. The process-lock lane requires the forced termination, shared namespace and native IO source regressions; its child-process helper does not satisfy coverage. Captured pretty harness output keeps nested trybuild diagnostics from splitting result lines. Zero-test, ignored, failed and name-lookalike output cannot satisfy the gate. When adding a required ownership test, update the typed suite inventory and its validator regressions; do not replace execution evidence with a successful Cargo exit status.

The VM lifecycle lane also binds each required session-owner and runtime-admission
doctest to its exact rustdoc item and source line. Discovery must contain the
expected number of unique guards, and every discovered guard must execute
successfully. Duplicate listings, similarly named items, ignored/failed results
and an empty successful harness cannot satisfy this lane. Update its typed
inventory deliberately when changing the required guard set.

Run `just ci-policy-toolkit-clippy` after changing repo-local validators. It runs
all-target Clippy with warnings as errors against `toolkit/xtask/Cargo.toml`.
`just ci-clippy` includes this command after the main workspace lint, since this
crate is excluded from that workspace. Keep policy predicates equivalent when
repairing lint findings and compile shared regexes once outside file loops.

The `signed-transcript-boundary` check is available independently for focused
security diagnosis. Guardian owned verifier byte origins are proved by
`guardian_transcript_scope`: declared typed transcript, required canonical
encoding, immutable primitive input, and exact rooted source dispatch. Its
actual-source regression rejects raw/fallback/shadowed byte origins and missing
typed declarations. Run the full `security-boundary-policy` after this focused
check; passing one constituent check does not prove the aggregate.
