# Hello World Guide

This guide gets you running with Aura in 15 minutes. You will build a simple ping-pong protocol, deploy it locally, and interact with it using the CLI.

## Setup

Aura uses Nix for reproducible builds. Install Nix with flakes support.

Enter the development environment:

```bash
nix develop
```

This command activates all required tools and dependencies. The environment includes Rust, development tools, and build scripts.

Cargo and `cargo clippy` both use the shell's pinned Rust toolchain. The shell
dispatches Clippy explicitly because Cargo can otherwise select an older
plugin installed in Cargo home before searching PATH. This requires no global
toolchain changes. `just ci-build-cache-policy` tests the dispatch, argument
boundaries, and forwarded exit status.

Build the project:

```bash
just build
```

The build compiles all Aura components and generates the CLI binary. This takes a few minutes on the first run.

For a deployable terminal binary, run `just build-release`. This builds the
production terminal feature set and installs `bin/aura` atomically. Use
`just build-dev` for the development feature set, and
`just build-workspace-release` when validating every workspace crate in the
release profile. The browser bundle has its own Dioxus build path.

On a clean fixed-commit comparison (`a9fabe9a`, macOS ARM), the terminal
release retained 1,825,040 KiB versus 3,068,204 KiB for the full workspace:
1,243,164 KiB (40.5%) less release output. The terminal compiled 330
packages versus 498 for the workspace. Shared warm caches vary with later
web, development and test builds; use the disk report to measure the current
checkout.

Repeated LAN test builds should use `just e2e-build-terminal`,
`just e2e-build-web`, and `just e2e-build-harness` in the Nix environment on
each host. These commands report free space, preview and collect idle Cargo
artifacts when needed, and stop their own build if free space reaches the
emergency floor. `just disk-report`, `just cache-inventory`, and
`just build-budget-dry-run` are read-only. Run the LAN recipes from the same
pushed commit on both hosts, in the background and one host at a time. Set
`CARGO_BUILD_JOBS` to half the available cores on the Air. The recipes use
`nice -n 10`. Use `AURA_BUILD_TARGET_CAP_GIB`, `AURA_BUILD_MIN_FREE_GIB`, and
`AURA_BUILD_EMERGENCY_FREE_GIB` to adjust Host B thresholds after its own
read-only baseline. The 10 GiB per-checkout Cargo target cap is a between-build soft target;
it is not a quota during compilation. The scripts preserve `.tmp/e2e` and failure
evidence. Use `scripts/dev/retain-e2e-runs.sh prune --dry-run` to inspect
completed successful run bundles, then `--apply` when no harness run is
active. Failed, pinned, active and unclassified bundles are preserved.

The disk report lists the current checkout and linked Git worktrees
separately, including debug incremental and trybuild caches; it never
collects another worktree's target.
Work 8 debug, test, and Clippy rebuilds also produce Cargo artifacts. Route
each of those commands through the same guard, keeping any chosen Cargo
profile or explicit incremental override on the command. During an
approved live LAN run, use:

```bash
AURA_BUILD_PROFILE=debug nice -n 10 bash scripts/dev/build-budget.sh \
  --lane work8-debug --allow-live-harness -- cargo test -p hxrts-aura-agent --lib
```

Replace the Cargo command after `--` for `cargo check`, `cargo clippy`, or
another package test. Omit `--allow-live-harness` outside a live run. The
guard refuses admission when another builder of the same checkout is active,
or when free space minus other admitted builds' reservations is below
the configured floor; wait for the current owner to finish and release an
idle cache window before retrying. The Cargo development profile disables
incremental compilation by default for ordinary Cargo and editor checks;
guarded builds also default to `CARGO_INCREMENTAL=0`. Opt in with
`CARGO_INCREMENTAL=1` when the saved rebuild
time justifies its retained cache. The fixed-source foundation measurement
retained 289,240 KiB without incremental compilation versus 552,508 KiB after
an incremental source rebuild (47.6% less). The wrapper's measured rebuild
times were 16s versus 12s; this is a foundation check, not a whole-workspace
performance guarantee. The wrapper and saved disk report log the effective
setting. Required cache-policy fixtures verify the wrapper behavior and compile
a tiny isolated package using the actual development profile to verify the
compiler default and explicit override.

Run `just ci-dry-run` only after Cargo, Dioxus and LAN harness consumers
have stopped. Its startup and per-step preflight refuses active consumers,
checks free space, and can collect only idle compiler caches. It preserves
`.tmp/e2e` and current CI logs.
A complete run needs about 40 GiB free at start. To exclude a step
explicitly, set `AURA_CI_DRY_RUN_SKIP` to comma-separated step names (for
example `"Tests + Protocol Compat"`); excluded steps are announced at the
start of the run.
If stale native release variants dominate after the LAN run ends, preview
`bash scripts/dev/prune-inactive-lane.sh --lane release --dry-run` and then
use `--apply` to reset that whole idle cache. The command refuses active
builders, harness processes and open release files. It leaves the installed
`bin/aura` alone; the next release build recompiles its dependencies.

The normal guarded build sequence on each host is:

```bash
just disk-report
just build-budget-dry-run
just e2e-build-terminal
just e2e-build-web
just e2e-build-harness
just disk-report
```

To compare the clean disk cost of the deployable terminal build with the
full workspace release gate, run
`scripts/dev/compare-release-scopes.sh --dry-run` first. On an idle host
with at least 40 GiB free, use `--apply`. The Bash script builds both scopes
from one commit in separate temporary worktrees, copies the disk reports to
`artifacts/disk-budget/comparisons/`, and removes each temporary target before
starting the next build. By default it refuses to run while a build or LAN
harness consumer is active.
For a reserved window with a live LAN harness but no Cargo/Dioxus builders,
use `--check --allow-live-harness` and then `--apply --allow-live-harness`.
This mode still refuses another builder, uses four low-priority Cargo jobs by
default, and touches only the temporary worktree target. Override the job
count with `AURA_COMPARE_CARGO_JOBS` when the host needs a lower limit.
The comparison uses the budget wrapper's `--no-prune` mode because each
temporary target is removed after measurement; its free-space admission and
emergency stop remain active.
If a separate builder starts before the second scope, the script preserves
the completed first-scope measurements and stops. Rerun with
`--resume artifacts/disk-budget/comparisons/<run-directory>` from a later
no-builder window; it verifies the fixed commit and skips successful scopes.

The tracked LAN entry point is `scripts/harness/lan/build.sh`; its lane is
`terminal`, `terminal-live`, `terminal-dev`, `web`, `web-live`, `harness`,
or `harness-live`. Use the live variants during an approved active LAN run;
they preserve release and web caches and only collect an idle WASM debug
lane when the budget requires cleanup.
It prints the host and commit before building, enters `nix develop` when
needed, and accepts `AURA_EXPECT_COMMIT=<full-hash>` to reject a host on a
different revision or a checkout with uncommitted changes. Use its
`--dry-run` flag to preview the selected recipe.
Use `scripts/harness/lan/build.sh all` for the complete terminal, web and
harness build sequence. It requires a clean checkout, pins the initial commit
through every step, and stops at the first failure. Run this sequence on one
host at a time; it does not start an E2E run or modify the other host.
LAN browser startup requires the prebuilt harness-enabled bundle and fails if
it is missing or stale. It cannot quietly start an unguarded Dioxus build or
clear the serving cache; stop the run and repeat the guarded web build first.
On Host B, the macOS application firewall may need the newly signed
`bin/aura` authorized again after a rebuild, as described in `work/8.md`.
The build helper does not change firewall settings.

Run `just e2e-build-terminal-dev` separately when a test explicitly needs
the development feature set. Each successful guarded build also saves its
own disk report under `artifacts/disk-budget/`.
For one-off Work 8 Cargo test, check, or Clippy rebuilds, wrap the Cargo
command too. For example, inside `nix develop`:

```bash
AURA_BUILD_PROFILE=debug AURA_BUILD_FEATURES=work8-test \
  nice -n 10 bash scripts/dev/build-budget.sh \
  --lane work8-debug --allow-live-harness -- \
  cargo test -p hxrts-aura-app
```

Substitute the required Cargo subcommand and package selector. Start only
when no other Cargo or Dioxus build is active. The live option preserves
release/web/Dylint caches while a LAN run is active; the disk monitor still
stops only its own command at the emergency floor.
When the LAN owner permits a terminal build during a live run, use
`just e2e-build-terminal-live`. That recipe skips global Cargo sweeping and
may collect only an idle WASM debug lane. The ordinary
`just e2e-build-terminal` recipe enforces the full idle-lane cache policy;
run it after the harness stops. Web and harness-tool rebuilds also require
the harness to stop before cache collection.
On Host B, pass its own `--root` ending in `/artifacts/runs` to the retention
script; it never prunes the other host remotely.
When only an unused debug lane can be released, inspect it with
`just prune-inactive-lane wasm-debug --dry-run` and then use `--apply`.
The cleanup checks for active compilers and open files in that lane.
Global Cargo sweeping also skips a target with open files, such as proc-macro
libraries loaded by `rust-analyzer`; it then considers only fully inactive
whole lanes. The 10 GiB per-checkout target is a soft between-build goal, so a lane held
open by another process may remain above it until that process exits.
`just prune-inactive-lane debug-incremental --dry-run` previews the complete
incremental cache independently of sibling debug dependencies. With `--apply`,
it refuses active compilers, open incremental files and symlinked parents,
and preserves release outputs and loaded debug libraries. The build and CI
guards consider this lane before the complete debug lane.

`just prune-inactive-lane trybuild --dry-run` separately previews the complete
`target/tests/trybuild` compile-fail cache. Its apply mode requires idle compilers,
no open files in that cache, and ordinary parent directories. Build and CI guards
can reclaim it while preserving editor-loaded sibling debug libraries. The next
compile-fail gate rebuilds this cache; its test sources and expected diagnostics
remain in the repository.

For an explicit browser release cache reset, `wasm-release` selects
`target/wasm32-unknown-unknown/wasm-release` and `wasm-host-release` selects
its host-side build cache at `target/wasm-release`. Preview each with
`just prune-inactive-lane <lane> --dry-run`. Apply refuses active compilers,
harness consumers, open files, and symlinked paths. These release lanes are
manual cleanup choices; automatic guards preserve them. Installed browser
bundles and run evidence stay outside these compiler cache paths.

To reproduce the debug cache measurement, run
`bash scripts/dev/compare-debug-incremental.sh --dry-run` first, then `--apply`
inside Nix during an idle build window. It checks the actual foundation crate
with incremental compilation enabled and disabled, measuring a clean check,
a warm check and a harmless source rebuild. It uses a fixed-commit source
archive and sequential disposable targets, retains logs under
`artifacts/disk-budget/debug-comparisons`, and retains its source snapshot on
failure. It leaves the main checkout's source, target and E2E evidence intact.
`just ci-build-cache-policy` runs the isolated Bash safety fixtures, including
default/override inheritance, active-build refusal, symlink containment,
fixed-commit sequencing, evidence retention and atomic installation. Both
GitHub Fast CI and `ci-dry-run` require this gate; it performs no real build
or shared-cache deletion.

## Creating an Agent

Aura provides platform-specific builder presets for creating agents. The CLI preset is the simplest path for terminal applications.

```rust
use aura_agent::AgentBuilder;

// CLI preset - simplest path for terminal applications
let agent = AgentBuilder::cli()
    .data_dir("~/.aura")
    .testing_mode()
    .build()
    .await?;
```

The CLI preset provides sensible defaults for command-line tools. It uses file-based storage, real cryptographic operations, and TCP transport.

For custom environments that need explicit control over effect handlers, use `AgentBuilder::custom()` with typestate enforcement. This requires providing all five core effects (crypto, storage, time, random, console) before `build()` is available.

Platform-specific presets are available for iOS (`AgentBuilder::ios()`), Android (`AgentBuilder::android()`), and Web/WASM (`AgentBuilder::web()`). These require feature flags to enable. See [Effects and Handlers Guide](802_effects_guide.md) for detailed builder examples.

See [Project Structure](999_project_structure.md) for details on the 8-layer architecture and effect handler organization.

## Ownership Declaration Before You Add New Parity-Critical Code

Before adding a new parity-critical module or workflow, declare its ownership
category in the crate `ARCHITECTURE.md`.

Use `Pure` for reducers, validators, and typed contracts. Use `MoveOwned` for handles, owner tokens, and ownership transfer. Use `ActorOwned` for long-lived mutable async state and coordinators. Use `Observed` for rendering, harness reads, and diagnostics.

Also declare which capability gates parity-critical mutation and publication, which module owns terminal lifecycle, and which timeout and backoff policy the owner consumes. If those points are not explicit, the new module is not ready to land.

## Hello World Protocol

Create a simple ping-pong choreography. This protocol demonstrates basic message exchange between two devices.

```rust
use aura_macros::tell;
use aura_core::effects::{ConsoleEffects, NetworkEffects, TimeEffects};
use aura_core::time::PhysicalTime;
use serde::{Serialize, Deserialize};

/// Sealed supertrait for ping-pong effects
pub trait PingPongEffects: ConsoleEffects + NetworkEffects + TimeEffects {}
impl<T> PingPongEffects for T where T: ConsoleEffects + NetworkEffects + TimeEffects {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping {
    pub message: String,
    pub timestamp: PhysicalTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pong {
    pub response: String,
    pub timestamp: PhysicalTime,
}

tell! {
    #[namespace = "hello_world"]
    protocol HelloWorld {
        roles: Alice, Bob;

        Alice[guard_capability = "hello_world:send_ping", flow_cost = 10]
        -> Bob: SendPing(Ping);

        Bob[guard_capability = "hello_world:send_pong", flow_cost = 10, journal_facts = "pong_sent"]
        -> Alice: SendPong(Pong);
    }
}
```

The choreography defines a global protocol. Alice sends a ping to Bob. Bob responds with a pong. [Guard capabilities](106_authorization.md) control access and flow costs manage rate limiting.
Outside the choreography DSL boundary, first-party Rust code should use typed capability families or `capability_name!` rather than hand-written capability strings.

Implement the Alice session:

```rust
pub async fn execute_alice_session<E: PingPongEffects>(
    effects: &E,
    ping_message: String,
    bob_device: aura_core::DeviceId,
) -> Result<Pong, HelloWorldError> {
    let ping = Ping {
        message: ping_message,
        timestamp: effects.current_timestamp().await,
    };

    let ping_bytes = serde_json::to_vec(&ping)?;
    effects.send_to_peer(bob_device.into(), ping_bytes).await?;

    let (peer_id, pong_bytes) = effects.receive().await?;
    let pong: Pong = serde_json::from_slice(&pong_bytes)?;

    Ok(pong)
}
```

Alice serializes the ping message and sends it to Bob. She then waits for Bob's response and deserializes the pong message. See [Effect System](103_effect_system.md) for details on effect-based execution.

## Local Deployment

Initialize a local Aura account:

```bash
just quickstart init
```

This command creates a 2-of-3 threshold account configuration. The account uses three virtual devices with a threshold of two signatures for operations.

Check account status:

```bash
just quickstart status
```

The status command shows account health, device connectivity, and threshold configuration. All virtual devices should show as connected.

Run quickstart smoke checks:

```bash
just quickstart smoke
```

This command runs a local end-to-end smoke flow (init, status, and threshold-signature checks) across multiple virtual devices.

## CLI Interaction

The `aura` CLI operates on the same account as the TUI. It reads the account from `--data-dir`, falling back to the TUI's default location (`$AURA_PATH/.aura`, or `~/.aura`). Run `aura --help` or `aura COMMAND --help` to see the available commands.

View the account:

```bash
aura --data-dir ~/.aura status
```

This prints the account's authority, nickname, threshold, device count and contact count.

Inspect and use chat, contacts, invitations and homes. Channels are named by name or id:

```bash
aura chat list
aura chat send general "hello"
aura chat history general --limit 20
aura contact list
aura invite create --invitee AUTHORITY      # prints a shareable code
aura invite import --code CODE
aura invite accept --invitation-id ID
aura home create --name Home
aura home invite AUTHORITY
aura slash "/topic welcome" --channel general
```

Every account command runs the same `aura_app::ui::workflows` functions as the TUI and web, through one typed request model.

### Driving a node from a program

`aura rpc` keeps one runtime online and reads one JSON request per line on stdin; `aura serve` keeps it online without a client:

```bash
aura rpc
{"id":1,"method":"chat_send","params":{"channel":"general","message":"hi"}}
{"id":2,"method":"subscribe","params":{"topics":["messages"]}}
```

A running node, the TUI or `aura serve`, also listens on the owner-only local socket `<data-dir>.sock` (beside the data directory, e.g. `~/.aura.sock`) (mode `0600`, same-user connections only, no network listener). Every `aura` account command and `aura rpc` first try that socket, so they work while the TUI holds the account. With no node running, the CLI opens the account's production runtime itself under the profile's exclusive lease.

The protocol schema is published at `crates/aura-terminal/schema/aura-rpc-v1.json`, generated from the request and response types and kept in sync by `just ci-rpc-schema`.

The first line written is a hello line with the protocol version, methods and event topics. Each response carries the request's `id` and the same `result` or `error` that `aura --json` prints. After `subscribe`, event lines (`{"type":"event","topic":"messages",...}`) interleave with responses; a subscriber that falls behind gets one `resync` event with a fresh snapshot. The session ends on EOF or `{"method":"shutdown"}`.

Command output is printed plainly. Add `-v` to also print runtime diagnostics.

For scripts, the global flags make every command machine-readable:

```bash
aura --json status            # {"ok":true,"result":{...}} on stdout
aura --yes chat leave general  # confirm a destructive command without a prompt
aura --timeout 30 sync once --peers PEER
```

Under `--json`, stdout carries exactly one JSON document, either `{"ok":true,"result":...}` or `{"ok":false,"error":{"code":...,"message":...}}`; diagnostics go to stderr. Destructive commands prompt on a terminal and fail without `--yes` otherwise. The exit code classifies failures: 0 success, 1 failed, 2 invalid input, 3 not found, 4 permission denied, 5 timeout, 6 unavailable.

## Testing Your Protocol

Create a test script for the hello world protocol:

```rust
use aura_macros::aura_test;
use aura_testkit::*;
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

#[aura_test]
async fn test_hello_world_protocol() -> aura_core::AuraResult<()> {
    // Create test fixture with automatic tracing
    let fixture = create_test_fixture().await?;

    // Create deterministic test effect systems
    let alice_effects = AuraEffectSystem::simulation_for_named_test_with_salt(
        &AgentConfig::default(),
        "test_hello_world_protocol",
        0,
    )?;
    let bob_effects = AuraEffectSystem::simulation_for_named_test_with_salt(
        &AgentConfig::default(),
        "test_hello_world_protocol",
        1,
    )?;

    // Get device IDs for routing
    let alice_device = fixture.create_device_id();
    let bob_device = fixture.create_device_id();

    let ping_message = "Hello Bob!".to_string();

    // Run protocol sessions concurrently
    let (alice_result, bob_result) = tokio::join!(
        execute_alice_session(&alice_effects, ping_message.clone(), bob_device),
        execute_bob_session(&bob_effects, ping_message.clone())
    );

    assert!(alice_result.is_ok(), "Alice session failed");
    assert!(bob_result.is_ok(), "Bob session failed");

    let pong = alice_result?;
    assert!(pong.response.contains(&ping_message));

    Ok(())
}
```

This test creates deterministic, seeded effect systems for Alice and Bob using `simulation_for_named_test_with_salt(...)`. The identity + salt pair makes failures reproducible. For comprehensive testing approaches, see [Testing Guide](804_testing_guide.md).

Run the test:

```bash
cargo test test_hello_world_protocol
```

The test validates protocol correctness without requiring network infrastructure. Mock handlers provide deterministic behavior for testing.

## Understanding System Invariants

System invariants are defined in [System Architecture](001_system_architecture.md) and [Theoretical Model](002_theoretical_model.md). Key invariants include Charge-Before-Send, CRDT Convergence, Context Isolation, and Secure Channel Lifecycle.

See [Project Structure](999_project_structure.md#invariant-traceability) for traceability details. When developing, ensure your protocols respect these invariants to maintain system integrity.

## Next Steps

You now have a working Aura development environment. The hello world protocol demonstrates basic choreographic programming concepts.

Continue with [Effects and Handlers Guide](802_effects_guide.md) to learn about effect systems, platform implementation, and handler patterns. Learn choreographic programming in [Choreography Guide](803_choreography_guide.md). For session type theory, see [MPST and Choreography](110_mpst_and_choreography.md).

Explore testing and simulation in [Testing Guide](804_testing_guide.md) and [Simulation Guide](805_simulation_guide.md).
