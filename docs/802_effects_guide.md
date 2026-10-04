# Effects and Handlers Guide

This guide covers how to work with Aura's algebraic effect system. Use it when you need to extend the system at its boundaries: adding handlers, implementing platform support, or creating new effect traits.

For the full effect system specification, see [Effect System](103_effect_system.md).

Keep native runtime construction failures source-bearing through preset builders.
Use `BuildError::RuntimeConstructionSource` for an underlying error rather than
copying its display text into `RuntimeConstruction`. A declared provider must
reach its actual effect owner; typestate presence alone does not verify wiring.
Exercise injected providers through the built runtime when testing composition.
Required effect initialization uses `BuildError::EffectInitSource`, retaining the
effect name and original cause through the native agent boundary. Directory
creation and signal registration failures must not become string-only config
errors. Plain configuration rejection can retain its explicit diagnostic shape.

AMP lifecycle failures retain concrete effect causes through the native runtime boundary. Canonical checkpoint absence has a private producer in the AMP journal reader. Scoped duplicate diagnostics require an exact requested entity and an independent successful canonical read before reconciliation; diagnostic wording and error records alone cannot suppress mutation failures. `AmpChannelError` carries source-bearing `AuraError` values and no longer promises equality; compare typed variants or stable categories. Foreign diagnostics explicitly discard native causes only at the presentation adapter.

## Preserve Concrete Error Causes

Required AMP state, bootstrap and participant reads use the native bridge
error contract. Return absence only after a successful canonical checkpoint
read proves exact context/channel absence. Participant augmentation uses the
required channel invitation reader; the general best-effort invitation listing
is an observed diagnostic surface and cannot supply authoritative membership.

Retain the native cause when retrying AMP sends. Publish an initial bounded-call
failure through the existing semantic owner, and classify transport outcomes
from typed native kinds and timeout-budget variants. Clock unavailability is
an unavailable dependency; only deadline evidence yields a timeout. Error
display text must not decide retry policy or terminal failure codes.

Use the existing typed conversions into `AuraError` when wrapping effect
failures. For cryptographic context, use `AuraError::crypto_with_source` with
the original error in an `Arc`; avoid reducing the cause to its display text.
If adding an error wrapper that stores an `Arc<dyn Error + Send + Sync>`, make
its standard `Error::source` return the wrapped error through `as_deref()`.
Verify this with a concrete `downcast_ref` assertion after cloning and a nested
source-chain assertion. Checking only source presence or display text cannot
detect an extra shared-storage wrapper. The serialization contract in
[Effect System](103_effect_system.md#typed-error-sources) omits these process-local
sources; use typed codes for cross-process failures.

## 1. Code Location

A critical distinction guides where code belongs in the architecture.

Single-party operations go in `aura-effects`. These are stateless, context-free handlers that take input and produce output without maintaining state or coordinating with other handlers.

Examples:
- `sign(key, msg) -> Signature` - one device, one cryptographic operation
- `store_chunk(id, data) -> Ok(())` - one device, one write
- `RealCryptoHandler` - self-contained cryptographic operations

Multi-party coordination goes in `aura-protocol`. These orchestrate multiple handlers together with stateful, context-specific operations.

Examples:
- `execute_anti_entropy(...)` - orchestrates sync across multiple parties
- `CrdtCoordinator` - manages state of multiple CRDT handlers
- `GuardChain` - coordinates authorization checks across sequential operations

If removing one effect handler requires changing the logic of how other handlers are called (not just removing calls), it belongs in Layer 4 as orchestration.

### Decision Matrix

| Pattern | Characteristics | Location |
|---------|-----------------|----------|
| Single effect trait method | Stateless, single operation | `aura-effects` |
| Multiple effects/handlers | Stateful, multi-handler | `aura-protocol` |
| Multi-party coordination | Distributed state, orchestration | `aura-protocol` |
| Domain types and semantics | Pure logic, no handlers | Domain crate |
| Complete reusable protocol | End-to-end, no UI | Feature crate |
| Handler/protocol assembly | Runtime composition | `aura-agent` |
| User-facing application | Has main() entry point | `aura-terminal` |

### Boundary Questions

Stateless code goes in `aura-effects`. Stateful code goes in `aura-protocol`. Single-party code goes in `aura-effects`. Multi-party code goes in `aura-protocol`. Context-free code goes in `aura-effects`. Context-specific code goes in `aura-protocol`.

## 2. Effect Handler Pattern

Effect handlers are stateless. Each handler implements one or more effect traits from `aura-core`. It receives input, performs a single operation, and returns output. No state is maintained between calls.

Production handlers (like `RealCryptoHandler`) use real libraries. Mock handlers (like `MockCryptoHandler` in `aura-testkit`) use deterministic implementations for testing.

See [Cryptographic Architecture](100_crypto.md) for cryptographic handler requirements.

### Implementing a Handler

Step 1: Define the trait in `aura-core`.

```rust
#[async_trait]
pub trait MyEffects: Send + Sync {
    async fn my_operation(&self, input: Input) -> Result<Output, EffectError>;
}
```

Step 2: Implement the production handler in `aura-effects`.

```rust
pub struct RealMyHandler;

#[async_trait]
impl MyEffects for RealMyHandler {
    async fn my_operation(&self, input: Input) -> Result<Output, EffectError> {
        // Implementation using real libraries
    }
}
```

Step 3: Implement the mock handler in `aura-testkit`.

```rust
pub struct MockMyHandler {
    seed: u64,
}

#[async_trait]
impl MyEffects for MockMyHandler {
    async fn my_operation(&self, input: Input) -> Result<Output, EffectError> {
        // Deterministic implementation for testing
    }
}
```

### Adding a Cryptographic Primitive

1. Define the type in `aura-core` crypto module
2. Implement `aura-core` traits for the type's semantics
3. Add a single-operation handler in `aura-effects` that implements the primitive
4. Use the handler in feature crates or protocols through the effect system

## 3. Platform Implementation

Use the `AgentBuilder` API to assemble the runtime with appropriate effect handlers for each platform.

### Builder Strategies

| Strategy | Use Case | Compile-Time Safety |
|----------|----------|---------------------|
| Platform preset | Standard platforms (CLI, iOS, Android, Web) | Configuration validation |
| Custom preset | Full control over all effects | Typestate enforcement |
| Effect overrides | Preset with specific customizations | Mixed |

### Platform Presets

```rust
// CLI
let agent = AgentBuilder::cli()
    .data_dir("~/.aura")
    .build()
    .await?;

// iOS (requires --features ios)
let agent = AgentBuilder::ios()
    .app_group("group.com.example.aura")
    .keychain_access_group("com.example.aura")
    .build()
    .await?;

// Android (requires --features android)
let agent = AgentBuilder::android()
    .application_id("com.example.aura")
    .use_strongbox(true)
    .build()
    .await?;

// Web/WASM (requires --features web)
let agent = AgentBuilder::web()
    .storage_prefix("aura_")
    .build()
    .await?;
```

### Custom Preset with Typestate

```rust
let agent = AgentBuilder::custom()
    .with_crypto(Arc::new(RealCryptoHandler::new()))
    .with_storage(Arc::new(FilesystemStorageHandler::new("~/.aura".into())))
    .with_time(Arc::new(PhysicalTimeHandler::new()))
    .with_random(Arc::new(RealRandomHandler::new()))
    .with_console(Arc::new(RealConsoleHandler::new()))
    .build()
    .await?;
```

All five required effects must be provided or the code won't compile.

### Required Effects

| Effect | Purpose | Trait |
|--------|---------|-------|
| Crypto | Signing, verification, encryption | `CryptoEffects` |
| Storage | Persistent data storage | `StorageEffects` |
| Time | Wall-clock timestamps | `PhysicalTimeEffects` |
| Random | Cryptographically secure randomness | `RandomEffects` |
| Console | Logging and output | `ConsoleEffects` |

### Optional Effects

| Effect | Default Behavior |
|--------|-----------------|
| `TransportEffects` | TCP transport |
| `LogicalClockEffects` | Derived from storage |
| `OrderClockEffects` | Derived from random |
| `ReactiveEffects` | Default reactive handler |
| `JournalEffects` | Derived from storage + crypto |
| `BiometricEffects` | Fallback no-op handler |

### Platform Implementation Checklist

- [ ] Identify platform-specific APIs for crypto, storage, time, random, console
- [ ] Implement the five core effect traits
- [ ] Create a preset builder (optional)
- [ ] Add feature flags for platform-specific dependencies
- [ ] Write integration tests using mock handlers
- [ ] Document platform-specific security considerations
- [ ] Consider transport requirements (WebSocket, BLE, etc.)

## 4. Testing Handlers

Test handlers using mock implementations from `aura-testkit`.

```rust
use aura_testkit::*;

#[aura_test]
async fn test_my_handler() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;

    // Use fixture.effects() to get mock effect system
    let result = my_operation(&fixture.effects()).await?;

    assert!(result.is_valid());
    Ok(())
}
```

Never use real system calls in tests such as `SystemTime::now()` or `thread_rng()`. Use deterministic seeds for reproducibility. Test both success and error paths.

See [Testing Guide](804_testing_guide.md) for comprehensive testing patterns.

## 5. Effect System Architecture

For deeper understanding of the effect system architecture, see:

- [Effect System](103_effect_system.md) - Full specification
- [Cryptographic Architecture](100_crypto.md) - Crypto handler requirements
- [System Architecture](001_system_architecture.md) - Layer boundaries

### Key Concepts

The effect system uses three layers:

1. Foundation effects in `aura-core` cover crypto, storage, time, random, console, and transport.
2. Infrastructure effects in `aura-effects` provide production handlers implementing foundation traits.
3. Composite effects are built by composing foundation effects. For example, `TreeEffects` combines storage and crypto.

All impure operations (time, randomness, filesystem, network) must flow through effect traits. Direct calls break simulation determinism and WASM compatibility.

Run `just check-arch` to validate effect trait placement and layer boundaries.

### Preserving workflow failure causes

Pass the concrete error into `runtime_call`, `journal_op`, `fact_encoding`,
or `ceremony_op`. These helpers require `Error + Send + Sync + 'static`;
passing `to_string()`, a borrowed error, or a display-only value is rejected.
Use a typed domain precondition variant when no underlying failure exists.
Do not invent a source to make a diagnostic message satisfy the helper bound.

Workflow conversion retains the `WorkflowError` as the standard error source,
followed by the context wrapper and original concrete cause. `WorkflowError::Core`
passes through unchanged. Time failures likewise retain `TimeUnavailable` and
the original runtime/query/parity cause. Inspect `Error::source()` and downcast
when selecting typed policy; display text is for diagnostics.

Compatibility: helper callers must pass owned concrete errors. Time failure
variants now include an owned `source` field; callers matching their kind use
`{ .. }`, and constructors must retain the actual failure. Existing outer
workflow categories and diagnostic display prefixes are retained; the journal
encoding path no longer adds a redundant serialization wrapper. Serialized
`AuraError` values omit process-local sources, so deserialization cannot recover
a typed cause for authorization or retry decisions.

For required canonical-state queries, propagate `Err` before issuing a
mutation or publishing readiness. Only `Ok(false)` means that the canonical
entity is absent. A failed query cannot justify creating or joining an entity.
If a failed operation triggers a second canonical read to distinguish an
already-completed operation from failure, that second read is also required;
do not replace its error with `false` or a stale projection.

### Native runtime error classification

Keep `RuntimeBridgeError` and its original source on native paths. Use the
runtime's structural invitation failure reason when deciding whether acceptance
was already handled or when publishing a contact confirmation failure. A
non-pending invitation is not necessarily accepted: revoked, expired, and
declined states must remain failures. Never infer these decisions from display
text, even when a message is stable or comes from a wrapped error.

When adding a native domain reason, match the concrete runtime cause
exhaustively at the bridge normalization boundary and test both genuine typed
causes and diagnostic lookalikes. Workflow context errors retain that native
cause through `Error::source()`. Converting to the existing foreign
`IntentError` or `CallbackError` payload is an explicit terminal diagnostic
operation and cannot feed native retry or authorization policy.

## Preserving timeout causes

For enrollment execution, derive the budget from the original registered
ceremony window before creating bounded child attempts. Restarting a task or
retrying transport must not create a fresh acceptance window. A pending
terminal state is a wait under that budget; it is not deadline evidence.

Use the required trusted-parent metadata reader for cryptographic admission.
Propagate secure retrieval errors and retain codec sources. Validate participant
count, uniqueness and signing policy before using the verifier. Query canonical
package presence through the fallible provider API first. A successful absence
permits legacy layout lookup; failed presence or byte reads must propagate.
Present but corrupt canonical bytes cannot be repaired with legacy bytes.
Observational `Option` readers cannot establish parent trust.

At native required-read adapters, classify concrete error variants before
building display diagnostics. Preserve the original wrapper and source chain.
Known storage, codec, authorization and network causes keep their own category;
generic internal wrappers may expose a more specific typed source. Required
clock failure, rollback and retry-attempt exhaustion are service failures, not
observed budget deadlines. Foreign diagnostic text cannot supply that evidence.

Use `TimeoutBudgetError::time_source_failure(error)` when a required clock operation returns an actual error. The detail-only constructor is for diagnostics without an underlying failure. Match typed timeout variants or stable codes; do not compare source-bearing errors for equality or infer deadline expiry from display text. Forward timeout and retry wrappers as errors so their standard source chains remain available. Serialized diagnostics carry no native source evidence.

### Preserve timeout ownership across awaits

Use the existing timeout budget or its child budget for subsequent stages. Clone shares observation and exhaustion; constructing a fresh budget from a prior duration discards the owner contract. Required runtime sleep returns a result: propagate its original source through the owned failure path. Check required time after either awaited branch and after retry backoff. Keep observation guards lexical and release them before awaiting. Cover rollback after progress, cloning, child deadlines, canceled waits, and validated restart with injected clocks. Compile-fail coverage prevents Copy reconstruction; semantic and native classification must exhaustively match timeout-budget variants.

### Persist enrollment windows through their sealed owner

Use `registered_enrollment_window` for an issued generation and the admitted-window constructor for a verified invitee manifest. Pass the resulting `EnrollmentWindow` through nested attempts. Its child, retry, and executor methods checkpoint the original parent observation before continuation. Do not reconstruct a duration or call a raw/no-op executor on that path. A new immutable user-transfer admission allocates its original window; existing admissions require the retained record. Missing legacy state requires fresh setup transfer rather than repair from raw identifiers.

Use fallible owned interval callbacks for required maintenance. Preserve the original error through task supervision, service health consumption, and shutdown. Run `just ci-ownership-policy`, `just ci-annotation-ratchet`, and the checkpoint/rollback source regressions after changing these boundaries.

### Using shared window arithmetic

Use `aura_core::types::window::{WindowInterval, WindowPosition, PhysicalMillis, ReceiptGeneration}` for validated interval arithmetic. Keep owner-specific admission and checkpoint APIs separate: interval deserialization validates bounds but grants no provenance. Construct physical execution through `TimeoutBudget`; its millisecond policy rejects empty or sub-millisecond windows. The flow-window owner should adopt `WindowInterval<ReceiptGeneration>` when its generation-window module integrates, reject unrepresentable endpoints, and keep checked epoch/base progression in its existing authoritative path. Keep physical clock fields out of flow receipts. Adoption of the shared arithmetic primitive is a separate integration step; existing flow-window code has not been migrated by this change. A subtraction-based membership implementation may admit base `u64::MAX` with extent one, whereas the checked primitive rejects its unrepresentable exclusive endpoint. Handle this as typed exhaustion or define a reviewed wider coordinate contract; do not silently clamp it.

### Recovering enrollment window phases

Retain the original immutable allocation, initial clock, canonical registration and immutable live-boundary marker before inserting the live registry entry. Recover an interrupted pre-live clock from the secure original allocation; never rebuild its duration from current time. Treat either canonical registration or the live marker as evidence that a missing clock cannot be repaired. For older records, mint a missing live marker only after both secure allocation and original clock validate. Preserve original clock/exhaustion identity in the execution capability and hold allocation continuity until required checkpoint storage acknowledges.

### Construct native profile writers before accessing secrets

Acquire `FilesystemProfileStorageHandler::acquire_owned_native` once for the selected physical profile. Attach its concrete `Arc<OwnedProfileLease>` to ordinary storage and pass the same owner into `ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner` for an explicitly permitted filesystem provider. This factory opens `secure_store` relative to the retained profile descriptor before reading or immutably admitting the wrapping key. Keep the returned owned provider intact; rebuilding a backend or generating a replacement for an invalid persisted key breaks provider continuity.

Native Unix file operations retain directory descriptors through staging, `renameat` or no-replace `linkat`, `unlinkat`, and directory sync. `NOFOLLOW` rejects descendant symlinks. The production effect constructor selects this factory before wrapping-key access. Unowned convenience constructors remain test/simulation surfaces; they do not grant a lifetime profile ownership guarantee. A profile path may cease naming its original directory, while an already-owned descriptor still names that original inode.

After an interrupted publication, reread and validate the canonical record under the retained owner. A successful local replacement does not atomically update the account profile, pending enrollment handoff, and signing generation together; that requires their transaction/WAL owner.

### Recover encrypted storage without replacing its master key

Retain the original secure master key when reopening an encrypted namespace. If
it is absent, inventory ordinary records through the retained storage owner
before first-use key admission. Existing records require the original key;
return the native storage failure when inventory or key recovery fails. Generate
an immutable first-use key only for an empty namespace. A concurrent wrapper may
have published the original key during inventory, so reread that canonical key
before rejecting an existing namespace. Preserve ciphertext and avoid publishing
new records on recovery failure. Actual owned-provider regressions cover missing
and corrupt keys and independent wrappers sharing the admitted key.

### Ordering shared physical observations

Use the shared timeout owner's asynchronous observation lease around a required
physical read, budget update, and checkpoint acknowledgment. Release the lease
before awaiting the operation or timer. Runtime remaining/child/ACK paths and
bounded-execution initial and completion paths must use the same gate. Do not
nest lease acquisition by calling another observing helper while holding it.
Test a provider that captures an earlier timestamp then suspends: a cloned child
must wait before querying, and cancelling the suspended observer must release
the gate without moving the deadline.

### Held enrollment roster inputs

Obtain `prepare_authenticated_enrollment_rotation` from the actual effect owner and transferred setup before deriving the enrollment ceremony. Use its read-only roster/prestate accessors for packaging, then consume that same plan in `prepare_pinned_enrollment_rotation`. Preserve generation → tree lock order and keep the returned reservation through manifest retention and canonical registration. Do not acquire either gate recursively or rebuild an issuer participant from an absent tree leaf. Original cleanup recovery uses its negative custody path; live recovery additionally checks the fresh authenticated decision and original clock.

### Choosing secure publication

Use `secure_create_mutable` for the first allocation of records that an existing domain owner must later checkpoint. Check `Created` versus `AlreadyExists` and validate retained original state; an existing result does not mean caller bytes match. Use `secure_store_immutable` for first decisions and durable evidence that must not be overwritten or deleted by generic storage calls. Sealing an existing legacy record preserves its original payload. See [the effect contract](103_effect_system.md) for publication and protection guarantees.

Recover failed publication by retrieving and validating the actual original through the selected provider. Do not create replacement wrapping keys when prior encrypted records or provider allocation evidence exist. Treat an IndexedDB compare-and-swap conflict as a typed storage conflict; the domain owner decides a bounded recovery. Ordinary profile keys cannot address the infrastructure `secure_store` subtree, and ordinary clear does not remove secure evidence.

### Required participant envelope recovery

Select the participant from the validated signing context. Decrypt retained envelopes using required wrapping-key reads; never call a create-on-absence helper from decryption. Creation belongs to encryption under its actual material owner: use atomic absent-only publication and reread the stored winner. Missing or corrupt active policy, key material, or envelope codecs must preserve their native causes; layout compatibility may use fallible existence checks but must not retry a different layout after an arbitrary read failure.

### Required issuer signing context

Pass the sealed context selected by the actual local signing owner to required issuer-key readers. Initial issuance selects required active policy; later confirmation selects its original context through the retained issued-manifest owner while holding current membership custody. Do not search historical epochs or derive participant identity from encrypted envelope metadata. Revalidate explicit layout presence before selecting a same-context compatibility row, and preserve read/decode/decryption causes.

### Bounding enrollment caller futures

Use lexical `Box::pin` delegates at enrollment facade, whole-window selection,
VM attempt and notice boundaries when nested futures exceed the caller budget.
Keep the same capability, references, registered task and original time window;
allocation does not grant another execution owner. The normal-stack actual
enrollment fixture checks issuer and invitee caller futures against a 16 KiB
budget. Do not replace that regression with an increased executor stack size.

### Required VM disposal regression lane

Run `just ci-vm-session-lifecycle` after changing a VM backend, targeted session
close/reap, coroutine index ownership, worker acknowledgment or forced-drop
custody. `just ci-ownership-policy` includes the same lane. The Rust inventory
checks actual nonignored test declarations and published harness names before
running the dependency and runtime suites in sequence. A renamed, absent,
feature-excluded or ignored required fixture fails instead of producing a
successful zero-test result.

The dependency tests exercise targeted removal, same-role surviving sessions,
new-session dispatch, genuine scoped worker acknowledgment, actual poison and
epoch faults, natural terminal epoch preservation and cooperative deserialization
index continuity. Runtime tests exercise both real backends, original error
source chains, exact forced-drop owner retirement, stale-owner rejection and
cleanup before supervisor idle publication.

The dependency override is outside the workspace. This lane invokes its actual
manifest with the multi-thread feature and shares the selected target directory
with runtime validation. It does not run competing builds. Keep its original
release provenance and license together with the patch record; update Cargo/Nix
pins when replacing the override. Disposal acknowledgment is local cleanup;
signed protocol outcome and remote delivery retain their separate owners.

### Recovering a negative enrollment notification

Use the existing post-bootstrap registration recovery owner. Retire the failed
pending signing generation, then pass its independently retained issued control
to the tracker's Cancelled-only notice producer. Transfer that sealed owner into
the existing invitation-service task group. Keep required lookup bounded by the
local ingress window and actual signing/delivery bounded by the original retained
window intersected with signed manifest validity. A local ingress timeout cannot
renew original signing eligibility. Propagate required lookup, checkpoint, signer,
transport and teardown failures to the task supervisor. Do not replace the already
retained Cancelled first decision when its notification fails or expires.

### Implementing allocation lifetime providers

Keep the selected physical-profile handoff separate from ordinary secure storage.
Use bounded authenticated records, atomic publication and required directory or
transaction acknowledgment. Keep pending original birth material until its
handoff is acknowledged; recover that exact pending birth instead of generating
a replacement. Record a final handoff phase before returning custody so an empty
previously transferred profile cannot be mistaken for interrupted initialization.
Use original backend objects for decisions and retirement, and retain real
failure sources. See `docs/100_crypto.md` and `docs/103_effect_system.md`.


### Allocation lifetime provider ownership

Selected allocation lifetime factories and provider identities originate only
from the actual exclusive physical profile owner. Native crypto retirement
requires the held original negative decision capability; positive sealing
requires the original activation capability. A serialized allocation locator
is observation, never recovery or retirement authority. Trusted custom effect
implementations are an explicit provider boundary.

Run `just _policy-check check security-boundary-policy` before broader CI when
changing these seams. Its Rust syntax check covers factory references, UFCS
aliases and macro tokens, while private constructors, declaration attributes
and compile-fail guards enforce actual capability custody. Keep immutable
legacy secrets permanent; allocation tombstone ACK is not a physical or backup
erasure guarantee.

### Supplying custom runtime handlers

Complete `AgentBuilder::custom()` typestate with crypto, ordinary storage,
physical time, random and console handlers before building. Async and sync
construction retain those same shared providers. Ordinary storage is wrapped
by unified encrypted storage and cannot replace the secure profile owner. The
configured random handler must satisfy the trait's cryptographic entropy
contract; simulation mode does not substitute a different handler.

An explicit transport inventory is bounded to sixteen providers. Declaration
order selects the first established channel, otherwise the first provider,
before sending. A send failure does not try another provider or native route.
Receiving inspects configured providers in order and continues only on typed
`NoMessage`. Return `NoMessage` when there is no matching envelope; a blocking
provider can delay later entries. Required provider outages should retain a
concrete native cause. Use the L8 custom-provider regression harness to verify
actual dispatch, encrypted bytes, and source retention.

### Bounded final clock observation

Use `aura_core::time::timeout::acknowledge_with_timeout_budget` after the actual
resource owner has acknowledged completion. Pass the original `TimeoutBudget`,
the selected `PhysicalTimeEffects` provider, the required checkpoint callback,
and a synchronous publication callback. The helper races the whole observation
lease/read/checkpoint against `wait_until_physical_deadline`, polls the endpoint
first when both are ready, and drops losing futures before returning. Publication
runs with the original observation guard held and has no following await.

Wrappers must forward absolute waits to their configured provider. A current
read followed by relative sleep, or a delay based on cached high-water, cannot
bound a stalled current read. Unsupported providers fail explicitly. Native
providers use cancellable timers; the manual provider waits for actual test
clock progress. Test delayed registration, blocked observation, hung reads,
checkpoint delay, native timer faults and same-poll late completion with
`just _policy-check check absolute-time-observation`.
