# Aura Agent (Layer 6)

AMP lifecycle failures retain concrete effect causes through the native runtime boundary. Canonical checkpoint absence has a private producer in the AMP journal reader. Scoped duplicate diagnostics require an exact requested entity and an independent successful canonical read before reconciliation; diagnostic wording and error records alone cannot suppress mutation failures. `AmpChannelError` carries source-bearing `AuraError` values and no longer promises equality; compare typed variants or stable categories. Foreign diagnostics explicitly discard native causes only at the presentation adapter.

## Purpose

Production runtime composition and effect system assembly for authority-based identity management. Owns structured concurrency, service lifecycle, session ownership, effect registry, builder infrastructure, and choreography adapters.

## Scope

| Belongs here | Does not belong here |
|---|---|
| Runtime assembly and effect composition | New effect implementations (aura-effects) |
| Service actor lifecycle and supervision | Multi-party coordination (aura-protocol) |
| Session ownership and ingress routing | Application-level workflow logic (aura-app) |
| Choreography adapter wiring | Bridge schema transformations (aura-quint) |
| Builder infrastructure (CLI, iOS, Android, Web) | Stateless single-party handlers (aura-effects) |
| Runtime-owned service caches and plane fusion such as rendezvous descriptors and provider candidate fusion | Layer 5 fact semantics or route-free candidate derivation |
| Structured concurrency and task supervision | Imports from Layers 1-5 back into this crate |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Consumes | `aura-core` (L1) | Effect traits, domain types, crypto utilities |
| Consumes | `aura-effects` (L3) | Stateless handler implementations |
| Consumes | `aura-protocol` (L4) | Protocol coordination |
| Consumes | L2 domain crates | Journal, authorization, transport, etc. |
| Consumes | L5 feature crates | End-to-end protocols |
| Consumes | `aura-macros` (L2) | Ownership and service declaration macros |
| Produces | `AgentBuilder`, `AuraAgent` | Runtime entry points |
| Produces | `EffectContext`, `EffectRegistry` | Effect composition |
| Produces | `AuraEffectSystem` | Subsystems: Crypto, Transport, Journal |
| Produces | Service APIs | Session, Auth, Recovery, SyncManager |
| Produces | `RuntimeSystem`, `LifecycleManager` | Lifecycle management |

The enrollment setup possession bridge uses runtime-owned physical time and
cryptography and returns sealed possession evidence with typed errors. It does
not certify user transfer or a durable device binding. Enrollment issuance
requires the app-owned transfer pin, checks validity before rotation, and uses
its exact provisional authority/device. Persisted setup replay suppression and
acceptance-verifier binding remain required before full trust migration is
complete; replacing the issuance parameter alone does not prove those contracts.

## Key Modules

The public invitation acceptance facade boxes its private delegated future
before awaiting it. The caller retains lexical ownership and cancellation;
acceptance does not create a task merely to control stack allocation.
`invitation_acceptance_caller_future_is_bounded` enforces a 16 KiB caller
future budget. The simulator's required AMP lifecycle replay runs on the
default test stack and exercises this facade through real channel invitations.

- `core/`: Public API (`AgentBuilder`, `AuraAgent`, `AuthorityContext`).
- `builder/`: Platform-specific preset builders (CLI, iOS, Android, Web).
- `runtime/`: Service actors, subsystems, choreography adapters.
- `handlers/`: Service API implementations (auth, session, recovery, etc.).
- `reactive/`: Signal-based notification and scheduling.

### Enrollment VM admission owner

Enrollment parent inventory uses a bounded, fallible secure metadata reader.
Missing records, storage failures, malformed metadata and invalid participant
policies remain distinct failures. Canonical package presence is checked through
the fallible secure provider before considering a legacy layout. A present but
corrupt canonical package cannot fall back; failed presence or byte reads retain
their native category and cause. Baseline reduction and signature failures
retain concrete sources. The initiator derives its execution window from the
original tracker registration rather than the time its task starts.

Required interval callbacks use `spawn_try_interval_until_named` (or its local
counterpart) and return the concrete `AuraError`. Invalid interval policy fails
before callback execution. Callback and timer failures terminate the task as
`TaskFailed`, retain the first cause for `terminal_failure`, and fail group drain.
`Ok(false)` denotes intentional completion. Owning services must consume that
retained failure when reporting health; logs alone do not establish service
health. Aggregate drain includes descendants through the shared admission
registry and observes actual future drop before registration removal.

Native required-read normalization classifies actual `AuraError` variants and
typed budget failures through retained source chains. Known storage, validation,
permission, network, crypto and serialization failures do not become Internal.
An unavailable or rolled-back clock is a service failure; only the budget's
observed `DeadlineExceeded` establishes expiry. Diagnostic wording does not
select the category. The original wrapper and concrete source remain traversable.

`handlers/invitation/enrollment_vm_admission.rs` consumes sealed manifest and
local setup evidence for request validation and response signing. Ceremony
control frames verify the independently retained initiator key. Confirmation
waits on the tracker-owned terminal notifier and signs only a committed owner
decision; pending state has no timeout category. The caller retains its original
effect-backed budget. These frames authorize no global transport identity.
Successful quorum enrollment, revocation continuity, cross-process profile
ownership and complete native/browser restart coverage remain required.

## Invariants

- Enrollment issuance retains the exact user-transferred setup statement and
  digest in a runtime-owned versioned secure record before starting ceremony
  owners. Response verification requires that retained verifier and checks the
  physical initiator, subject, ceremony, pending epoch, invitee identity and
  proof policy. Missing legacy records fail closed; peer-supplied packages
  cannot replace the expected signer. This record is not a consumed setup nonce
  receipt or an atomic activation transaction, and does not authenticate the
  invitee's imported baseline or reverse initiator trust.

Summary:

- All production async work uses structured concurrency with explicit task ownership.
- External events reach session state only through typed ingress and the current owner.
- Invitation acceptance preserves concrete validation and contact confirmation
  failures through agent error conversion. Runtime normalization matches their
  actual variants and observed settled status into shared native reasons;
  message text cannot imply acceptance, revocation, expiration, or confirmation.
- Sibling fact exchange preserves codec, tree-operation, and frame-order causes as typed error categories with their original sources; contact acceptance preconditions and response signing likewise retain typed causes through agent error conversion.
- The enrollment invitee matches requests against its invitation-owned subject,
  ceremony, target device and pending epoch before VM injection. Confirmation
  must establish that exact invitation and epoch. Typed content mismatches and
  malformed serialization terminate the attempt without retrying them as a
  transient transport outage; authentication remains a separate ingress contract.
- Sibling tree ingress verifies each new operation with locally trusted parent-epoch key and threshold metadata, checks its exact parent state, and validates the whole extension before persistence. Missing local trust fails closed even for a first import or a single-signer tree. Enrollment baseline replacement requires a separate authenticated ceremony-scoped verifier chain; an invitation's pending-epoch package or a peer frame is not a trusted parent verifier by itself.
- A LAN receipt signed under the public key embedded in that same receipt proves payload integrity only. It cannot mint verified authority ingress; the expected authority/device key must come from an independently verified binding. Imported invitation trust distinguishes confirmed invitation-key continuity, an unbound device-key match, and self-certification so a contact relationship alone never certifies a claimed device.
- Each active session has exactly one local owner at any time.
- Enrollment setup export belongs to the threshold signing owner. It captures
  the actual runtime device and a locked signing-context snapshot, verifies its
  own possession proof, and retains the exact code before returning it. Frontends
  cannot supply a substitute device ID, key package, epoch or request nonce to
  this exporter. Retention alone does not consume a request or authenticate a
  physical-device association; ceremony replay and user-transfer trust are
  separate contracts.
- Signing bootstrap is serialized across cloned service handles and restores
  persisted active signing state. Corrupt epochs, inconsistent policies and
  partial prior bootstrap records fail without replacing keys. Share decryption
  reads its existing wrapping key and cannot generate one on a read failure.
- Signing export, bootstrap, preparation, commit and rollback share a lifecycle
  lock. Service commit validates the prospective context before persisting the
  active epoch and publishing memory state; local membership is recomputed rather
  than retained from an earlier participant set. Historical/active rollback,
  stale or substituted commit, and replacement of an existing pending package
  fail. A retained local threshold share must match its authenticated storage
  envelope, native FROST package, signer index, policy and group before context
  restoration or service activation.
- Bootstrap stores a bound pending genesis record before key persistence and
  marks completion only after verifying the exact signed device creation and its
  durable tree operation/index entries. The signing context is published after
  completion, so a failed leaf commit cannot become readiness on retry. A
  matching pending record can resume with retained keys; legacy storage requires
  an existing authenticated creation witness and never invents missing lineage.
- A consumed `OwnedVmSession` releases its runtime binding and fragment claims
  after terminal VM cleanup as well as explicit early close. Automatic VM reaping
  must not strand the admitted runtime owner; stale owner capabilities still fail
  before teardown. The bounded enrollment host and two-runtime exchange tests
  enforce progress and owner release.
- Runtime composition assembles existing handlers; it does not create new effects or protocol logic.
- Runtime telltale integration consumes bridge artifacts but does not redefine bridge schema.
- Certain ownership/session violations are fatal and unrecoverable.
- Authority-first design: all operations scoped to specific authorities.
- Lazy composition: effects assembled on-demand.
- Mode-aware execution: production, testing, and simulation use same API.
- For shared semantic flows, `aura-agent` is the primary `ActorOwned` crate. It may own long-lived mutable async runtime state, but it must not leak that ownership into frontend-local semantic lifecycle authorship.
- Mutable runtime service views such as rendezvous descriptors, provider health, selector state, and hold observations are owned by the actor-owned service registry in `src/runtime/services/service_registry.rs`.
- Bootstrap discovery is runtime-owned and separate from ordinary rendezvous peer state. Native LAN discovery and broker-backed browser startup both surface `bootstrap candidates`, but those candidates must not be published as ordinary peers until enrollment/acceptance completes.
- The local bootstrap broker keeps bearer material out of URLs. Invitation
  retrieval credentials are transported in headers, compared through the
  constant-time credential helper, and protected by explicit connection and
  request-read limits. Loopback remains the default bind policy; LAN binding is
  opt-in.
- The actor-owned runtime service set includes rendezvous descriptor selection for `Establish`, the bounded `MoveManager` for current movement queues, replay suppression, flush scheduling, and congestion state, and the `HoldManager` for shared custody, selector rotation, bounded holder residency, local GC, and verified-only accountability updates.
- Adaptive privacy runtime-owned services include `SelectionManager`, `LocalHealthObserver`, `CoverTrafficGenerator`, and `AnonymousPathManager`; they own local health smoothing, weighted selection, cover planning, and anonymous established-path lifecycle inside `aura-agent`.
- `src/adaptive_privacy_control.rs` owns the Telltale-native protocol
  definitions for adaptive-privacy control-plane execution only:
  `AnonymousPathEstablishProtocol`, `MoveReceiptReplyBlockProtocol`,
  `HoldDepositReplyBlockProtocol`, `HoldRetrievalReplyBlockProtocol`, and
  `HoldAuditReplyBlockProtocol`.
- Those adaptive-privacy control-plane choreographies remain theorem-pack-free
  until Aura has a dedicated runtime-admission surface beyond ordinary
  protocol-machine admission and the current local runtime executors have been
  removed.
- Bootstrap and stale-node re-entry remain runtime-local in `aura-agent`
  because they are still broker/hint lookup and cache-refresh logic, not
  canonical multi-party admission/evidence protocols.
- `LocalSelectionProfile` is runtime-local. It must not be published as authoritative shared state, surfaced through frontend-facing shared contracts, or mirrored into Layer 5 facts. The sanctioned query surface is the runtime-owned `ServiceRegistry` selection snapshot path.
- `SelectionManager` fuses `Neighborhood Plane` and `Web of Trust Plane`
  permit inputs with descriptor snapshots, local health, and runtime budgets.
  That fused policy remains runtime-local in `SelectionState` and
  `LocalSelectionProfile`; it does not become a shared trust tier or
  route-shaped descriptor field.
- Transparent adaptive-privacy routing uses one shared envelope family for `Move`, held-object deposit and retrieval, sync-blended retrieval, cover traffic, and accountability replies. Retrieval and accountability must not regain separate transport families or mailbox-shaped network semantics inside `aura-agent`.
- Harness and shared-flow lanes must remain independent of `transparent_onion`; transparent mode is debug/test/simulation-only and may not become a parity-critical dependency.
- `AnonymousPathManager` owns encrypted anonymous established-path lifecycle in
  production. `transparent_onion` may expose debug/simulation-only setup and
  envelope objects for inspection, but those objects stay quarantined to the
  explicit transparent debug surface and do not transfer ownership of adaptive
  policy or path selection away from the runtime-owned services.
- Contacts/friend projections derive `ContactRelationshipState` from relational facts inside `aura-agent`; frontend shells consume the emitted projection and do not keep separate friendship state machines.
- Runtime home projection creates a canonical home only from a `HomeCreationWitness` derived through `ProjectionOwner` from `SocialFact::HomeCreated`. The invitee first verifies its AMP checkpoint and local join, commits the creation fact, then binds that fact to opaque joined-home evidence before projecting. Accepted invitation evidence on the inviter only enriches an existing home; `MemberJoined` and moderation facts cannot create one. The home reducer buffers early membership, binds joins to both home ID and context, deduplicates replay, and counts only materialized members; a remote creator is not the local online member. The witness proves fact shape; journal ingestion and the invitation workflow own commitment/authenticity upstream.
- Runtime invitation projection creates rows only from `InvitationCreationWitness` supplied by a validated imported cache record or `InvitationFact::Sent`; status-only facts update existing rows but cannot fabricate one. Runtime contact projection creates rows only from `ContactAddedWitness` supplied by `ContactFact::Added`; friendship and guardian facts enrich established contacts, with early friendship state buffered until creation arrives.
- Runtime-owned service declarations should prefer the `#[actor_owned(...)]` layer where a service exposes a stable long-lived command/ingress boundary; changed-files ratchets in `just ci-ownership-policy` enforce this incrementally.
- Task-supervision service roots that do not expose a stable command-ingress surface should use `#[actor_root(...)]` instead of forcing the store-style `#[actor_owned(...)]` command-enum pattern.

### InvariantStructuredConcurrency

All production async work uses structured concurrency with explicit task ownership.

Enforcement locus:
- `src/runtime/` service actors own their task groups.
- `TaskGroup` enforces parent-child relationships.

Failure mode:
- Detached tasks outlive their owner and mutate torn-down resources.
- Shutdown leaves orphan tasks running.

Verification hooks:
- `just ci-actor-lifecycle`
- `just test-crate aura-agent`

Contract alignment:
- [Runtime](../../docs/104_runtime.md) defines service actor patterns.
- Actor services are the correct abstraction for runtime supervision; they are not the abstraction that defines session ownership transfer.

### InvariantCanonicalIngress

External events reach session state only through typed ingress and the current owner.

Enforcement locus:
- `SessionHandle` provides the only ingress path.
- Session actors consume ingress and drive protocol-machine work.

Failure mode:
- Direct session mutation from arbitrary tasks.
- Protocol-machine/session state diverges from canonical execution.

Verification hooks:
- `just ci-async-session-ownership`
- `just ci-choreo-parity`

Contract alignment:
- [Effect System](../../docs/103_effect_system.md) defines the session-local protocol-machine bridge.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines canonical execution.
- Session ownership is explicit and singular; delegation is modeled as a move, not as ambient access through a service actor.

### InvariantSessionOwnership

Each active session has exactly one local owner at any time.

Enforcement locus:
- Ownership transitions are explicit state machine transitions.
- Delegation commits atomically.

Failure mode:
- Overlapping owners mutate the same session.
- Stale owner access after delegation.

Verification hooks:
- `just ci-async-concurrency-envelope`
- `just ci-move-semantics`
- `just test-crate aura-agent`

Contract alignment:
- [Runtime](../../docs/104_runtime.md) defines session management.

### InvariantRuntimeCompositionBoundary

Runtime composition assembles existing effect handlers without introducing new effect implementations or protocol logic.

Enforcement locus:
- `src/runtime/` composes handlers and services through registry and builder types.
- `src/builder/` constrains runtime modes and dependency wiring.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just check-arch`
- `just test-crate aura-agent`

Contract alignment:
- [System Architecture](../../docs/001_system_architecture.md) defines layer boundaries.
- [Effect System](../../docs/103_effect_system.md) defines composition constraints.

### InvariantBridgeOwnershipAgent

Runtime telltale integration consumes bridge artifacts but does not redefine bridge schema.

Enforcement locus:
- `src/runtime/choreo_engine.rs` and `src/runtime/choreography_adapter.rs` enforce runtime capability admission.
- generated `CompositionManifest` theorem-pack metadata is translated exactly once by `aura-protocol::admission`, then consumed by `src/runtime/vm_host_bridge.rs` and `src/runtime/choreo_engine.rs`.
- `tests/telltale_machine_parity.rs` and `tests/telltale_machine_scenario_contracts.rs` run runtime parity and contract lanes for the admitted protocol-machine path.

Failure mode:
- Runtime layer duplicates schema translation code and drifts from `aura-quint`.
- Admission and parity behavior diverges across runtime profiles.

Verification hooks:
- `just ci-choreo-parity`
- `just ci-conformance-contracts`

Contract alignment:
- [Formal Verification Reference](../../docs/120_verification.md) defines runtime parity lanes.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines runtime admission guarantees.
- [Choreography Guide](../../docs/803_choreography_guide.md) defines the theorem-pack admission boundary and taxonomy.

### InvariantFatalViolations

Certain conditions are fatal runtime invariant violations:

- Ambiguous active session ownership.
- Delegated endpoint still usable by old owner after commit.
- Session-bound effect executed by non-owner.
- Protocol-machine/session mutation from outside canonical ingress.
- Runtime proceeding in a concurrency mode that has not been admitted.
- Teardown that leaves owned tasks mutating torn-down resources.
- Impossible typed state combinations.

Recovery is acceptable only when:
- The violating operation is rejected before state mutation.
- The failure is surfaced as a typed error and structured event.

## Structured Concurrency Model

`aura-agent` uses structured concurrency as the only production async model. This model is intentionally split:

- actor services solve long-lived runtime supervision and lifecycle
- move semantics solve session and endpoint ownership transfer

Do not collapse those into one generic async pattern.

Rules:

- Every long-lived async subsystem has one named owner.
- Every owner has one rooted task group.
- Child tasks belong to exactly one task group.
- Detached fire-and-forget tasks are forbidden in production runtime code.
- Shutdown is hierarchical and parent-driven.
- `src/runtime_bridge/` is also the L5/L6/L7 error normalization boundary:
  feature- and service-local failures may stay crate-specific internally, but
  bridge exports must classify frontend-visible failures into stable
  typed categories with operation context. Native acceptance, enrolled signer
  adoption, and time calls retain their original causes in `RuntimeBridgeError`.
  The foreign `IntentError` diagnostic enum remains unchanged and is produced
  only by an explicit terminal adapter. Remaining bridge methods still require
  migration to the native source-preserving boundary.
- Current `Move` traffic uses the shared transport envelope family plus the actor-owned `MoveManager`; social topology may influence admission and selection, but not the schema of the moved envelope itself.
- Current `Hold` traffic uses selector-based retrieval over one shared custody substrate. `HoldManager` owns held-object copies, reply-block bookkeeping, runtime-local indexes, and provider budgets; only verified witnesses may update `Hold` provider health or admission penalties.

See [Runtime](../../docs/104_runtime.md) §Service Actor Patterns for the actor struct examples, command/reply pattern, and async primitive preferred/forbidden lists.

### Concurrency Profiles

Three runtime concurrency profiles for choreography work:

- **Canonical**: Exact single-owner reference path (concurrency n=1).
- **EnvelopeAdmitted**: Disjoint or admitted work preserving safety-visible meaning.
- **Fallback**: Immediate degradation to canonical execution when envelope admission fails.

See [Runtime](../../docs/104_runtime.md) §Concurrency Profiles for the full contract, envelope admission rules, and current path classification.

## Session Ownership

Telltale-facing session state follows strict ownership rules. This is the move-semantics side of the runtime design.

Rules:

- Each active session or fragment has exactly one current local owner.
- The owner is either a per-session actor or an authoritative choreography runtime loop.
- Network, timer, and external events are queued before touching session state.
- Session ownership and task ownership move together.
- Session-bound effects execute only under the current owner capability.
- Runtime bridge query APIs fail explicitly when a required runtime service is
  absent; they do not return empty authoritative-looking peer/discovery results
  as a fallback.
- Runtime bridge lifecycle APIs that can distinguish `no progress`,
  `already running`, `processed`, or `degraded` must return typed outcomes
  rather than flattening those states into `Result<(), _>`.

The current owner may be hosted by an actor, but ownership itself remains a single-owner move boundary, not a shared mutable actor coordination pattern.

### Owner Record vs Owner Capability

- The owner record answers who currently owns the session or endpoint.
- The owner capability answers what that owner may currently do.
- Delegation must update both the owner record and the relevant capability state.
- A valid owner record without the required capability is insufficient.

### Effect Path Classes

Three ownership classes for runtime effect paths:

- `service-owned`: lifecycle, maintenance, discovery, shutdown, reactive scheduling.
- `session-owned`: protocol-machine/session mutation, blocked-receive injection, owner-routed round advancement.
- `capability-gated trust-boundary APIs`: commands crossing subsystem or authority boundaries requiring capability validation.

Service-owned effects never mutate session state directly. Session-owned effects require both current owner record and current owner capability. Capability-gated trust-boundary APIs fail closed on stale owner, stale capability, or wrong-boundary routing.

Production fail-closed rules are explicit:

- inbound transport receipts are validated before runtime consumers act on the envelope
- when multiple devices share one authority inbox, a choreography session may promote only frames addressed to its own device; frames for another device remain in the shared inbox until that device's session owner receives them
- owner capabilities must match the exact issued session, not merely owner label and generation
- delegation fails closed when source ownership was not recorded; the runtime does not backfill ownership as a repair path
- choreography receive timeouts are bound to issued timeout witnesses rather than reconstructed from later elapsed-time checks

Reactive signal views are `Observed` bridges, not alternate owners. They may
apply authoritative facts to known entities, but they may not fabricate
canonical channel or invitation metadata from weaker facts such as membership
events or raw identifiers. If runtime acceptance or reconciliation needs to
materialize canonical metadata, one explicit owned handler path must do that
end to end before reactive views are allowed to enrich the projection.
`ChatSignalView` stages pre-creation `ChannelUpdated` metadata without exposing
a channel. It consumes `CanonicalChannelCreation` from `ChannelCreated`, replays
staged updates in timestamp order, and keeps later duplicate creation from
resetting metadata. Runtime channel-name lookup likewise requires a matching
creation fact before an update-derived name can identify a channel.
The runtime bridge can recover a `CanonicalChannelCreation` for a joined
channel only from a committed `ChannelCreated` fact matching both the channel
and authoritative context; AMP membership and name hints are not creation
evidence. This closes the gap when the join result reaches the app before
reactive signal delivery.
The runtime's reactive views and invitation materializers publish through
`aura-app::projection_owner::ProjectionOwner`, which serializes each signal
commit and assigns a source revision. Pure materializer deltas update the
current signal in one transaction. Views that await decoding or effects must
recompute against a fresh snapshot and conditionally publish at its revision;
a stale batch retries until committed so canonical facts are not dropped. They must never emit
a whole snapshot derived from an older read. The source revision is distinct
from the frontend's render and semantic revision.
Fact commit is the completion boundary for converted runtime-owned invitation
acceptance and inbound transport materialization paths; those paths must not
wait for an uncorrelated "next reactive view update" before returning success.

Runtime bridge lookup follows the same strong-ref rule:

- context-scoped routing may use only descriptors bound to the requested
  context, not cross-context or "any descriptor" fallback
- channel-context answers must come from materialized runtime-owned context
  state, not invitation storage or local chat-fact repair
- name lookup may identify only already-materialized channels; it may not
  upgrade imported invitation metadata into an authoritative binding
- invitation-triggered home signal materialization must flow through the
  declared reactive home-signal owner path in `reactive/app_signal_views.rs`,
  not through handler-local signal read/patch/emit logic
- runtime bridge query APIs must fail explicitly when the required runtime
  service is absent; they may not silently degrade absence into empty success
  values that look like authoritative state
- storage-backed runtime bridge queries such as settings, device, or authority
  listing must also fail explicitly on storage/tree read errors; they may not
  synthesize default snapshots or current-only fallback rows that look like
  authoritative inventory
- invitation-backed runtime bridge queries must also fail explicitly when the
  runtime is no longer accepting public operations; they may not collapse
  `invitation_service` unavailability into empty pending/invited sets
- any other runtime bridge read that depends on invitation-owned derived state,
  including participant augmentation or settings contact counts, must follow
  the same rule instead of downgrading service loss into partial success
- authority listing must fail on account-config read failure or corrupt stored
  authority records; it may not publish a partial authority inventory that
  looks authoritative after skipping unreadable entries
- runtime bridge reachability checks must use only current-context transport or
  current-context descriptor evidence; they may not promote peer-default-
  context or LAN-discovery caches into an authoritative online answer
- peer-channel initiation must also use only descriptors bound to the requested
  context; it may not retry the peer's default context as a hidden fallback
- LAN descriptor publication must fail explicitly when rendezvous publication
  is denied or returns no descriptor payload; runtime startup and maintenance
  may not install a synthetic runtime-local descriptor as a repair path
- threshold key-rotation commit and consensus threshold-state loading must use
  the runtime-owned `threshold_config` written by the current rotation path;
  they may not resurrect legacy `threshold_metadata` blobs as a compatibility
  upgrade
- effect-backed threshold config/state queries must persist and read the same
  `threshold_config` record; they may not keep a second legacy storage schema
  alive behind the runtime-owned path
- runtime authentication queries must return one explicit status contract;
  they may not mutate journal state through a legacy `authenticate()` wrapper
  and then collapse the answer to a bare boolean
- device-threshold key-package envelopes must carry
  `metadata["target-authority-id"]`; the handler may not repair malformed
  envelopes by treating `TransportEnvelope.destination` as semantic target
- ceremony registration and supersession tracking must carry explicit
  `prestate_hash` bindings end to end; the tracker/runner may not admit
  optional prestate or compatibility wrappers that weaken supersession
  semantics
- enrollment attempt and retry helpers require effect-backed clock reads,
  retain clock/sleep sources, and derive child budgets from the owner window.
  Only actual deadline expiry enters timeout retry; invalid policy and
  unavailable clocks remain required failures. The `device_enrollment::budget_tests`
  and `invitation::vm_loop::tests` suites enforce this distinction.
- enrollment code issuance and ceremony completion have distinct outcomes.
  The runtime ceremony owner records one typed terminal outcome, preserves
  the first result under duplicate or late messages, and makes it available
  after restart. Deadline, authenticated refusal, cancellation, and commit
  must settle that owner; a spawned ceremony error may not be log-only.
- an invitee's local decline or acceptance cannot establish the initiator's
  terminal outcome by itself. Cross-runtime refusal and guardian completion
  require invitation-bound authenticated evidence before either side reports
  ceremony success or rejection.
- a guardian acceptance binds the inviter proof key from the imported code.
  The principal signs its post-verification confirmation with the retained
  local key matching that proof, including when its current identity epoch
  has rotated; an unavailable old key is an explicit failure.
- contact invitation acceptance completes only on the inviter's signed
  response (`handlers/invitation/contact_confirmation.rs`): the invitee
  materializes `ContactFact::Added` and marks the invitation accepted only on
  a confirmation that verifies against the imported code's sender-proof key
  and answers the digest of the acceptance it sent; a revoked, expired or
  settled invitation yields a typed rejection and no contact, and no response
  within the bounded wait is a typed failure that leaves it pending. The
  inviter authenticates an acceptance before answering, and re-confirms a
  duplicate from the same acceptor
- imported channel invitations and channel-acceptance notification must require
  the authoritative invitation context end to end; they may not default to the
  sender's home context when importing, loading, or establishing the sender
  peer channel
- runtime-owned relational-fact pull must fail explicitly when rendezvous is
  unavailable or no websocket-capable descriptor exists for the target peer; it
  may not downgrade missing transport reachability into a zero-fact success
- post-ceremony reachability refresh must fail explicitly when no sync service
  or no seeded sync peers are available; it may not report a successful refresh
  without any reachable peer evidence
- runtime ceremony transport delivery failures must use the transport/network
  error class directly; they may not string-match transport error text to
  synthesize semantic reachability narratives
- peer-channel setup must not report success after a channel becomes visible if
  the follow-on seeded sync step cannot run; established transport evidence and
  post-establishment sync remain one fail-closed completion contract
- peer-channel bounded convergence after initiation must also fail closed on
  sync or ceremony-processing errors; it may not suppress post-initiation
  failures and still report channel setup success

## Canonical Host/Protocol-Machine Boundary

`aura-agent` aligns with Telltale's canonical execution model. The only legal path from external async input to session mutation:

1. External event received by host runtime.
2. Host runtime converts it to typed ingress.
3. Ingress routed to the current authoritative owner.
4. Owner drives protocol-machine/session work at the sanctioned boundary.

Enforcement notes:

- Raw protocol-machine admission helpers stay inside the runtime boundary; higher layers use owned ingress/session wrappers.
- Protocol fragment ownership mutation stays inside runtime-owned surfaces.
- Link/delegate orchestration uses `ReconfigurationManager`; `ReconfigurationController` remains an internal runtime primitive.

See [Runtime](../../docs/104_runtime.md) §Link and Delegate Boundaries for the full link/delegate boundary contract, delegation bundle composition, and theorem-pack alignment rules.

## Telltale Bridge Ownership

- `aura-agent` owns runtime admission wiring and choreography backend selection.
- `aura-agent` owns telltale runtime parity test lanes and scenario contract gates.
- `aura-agent` must not own bridge schema transformations that belong in `aura-quint`.
- `aura-agent` uses Telltale `10.0.0` public semantic objects for authoritative reads, finalization paths, semantic handoffs, ownership receipts, reconfiguration snapshots, and runtime-upgrade artifacts; it must not recreate a private mirror of those concepts in parallel.

## Cross-Crate API Boundary

Other crates interact with `aura-agent` through sanctioned public APIs only.

- No direct imports of internal runtime modules from other crates.
- No bypass of `AuraAgent` or sanctioned public handles for runtime ownership-sensitive work.
- No cross-crate reach-in to mutate service or session internals.

Enforced by architectural policy gates and visibility rules.

## Ownership Model

Ownership categories follow [docs/122_ownership_model.md](../../docs/122_ownership_model.md).

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Runtime services and long-lived async coordinators | `ActorOwned` | service actor / rooted task group | owning service module and its typed command ingress | `aura-app`, frontends, harness |
| Session / endpoint / fragment transfer surfaces | `MoveOwned` | current owner record and capability scope | sanctioned delegation / transfer APIs only | projections, diagnostics, harness |
| Runtime-facing readiness and lifecycle state consumed by shared semantic flows | `ActorOwned` | runtime readiness/lifecycle coordinator | owning runtime coordinator and sanctioned hooks | `aura-app`, frontends, harness |
| Frontend-visible projections and facts | `Observed` | downstream of runtime/workflow ownership | projection reducers/exporters only | frontends, harness |
| Reducers, validators, typed contracts | `Pure` | compile-time | n/a | all layers |

### Concrete Boundary Map

- `ActorOwned`
  - `runtime/services/sync_manager.rs` via `SyncServiceManager`
  - `runtime/services/rendezvous_manager.rs` via `RendezvousManager`
  - service-local supervision rooted in `task_registry.rs`
- `MoveOwned`
  - `handlers/invitation.rs` via private-field, nonclone
    `ReservedInvitationIssuance`; reservation commits and sends nothing,
    consumption validates the physical issuer, and deadline overflow fails
    before fact preparation
    The preparation facade heap-pins its delegated stage, retaining lexical
    ownership and bounding its caller future to 16 KiB in a required regression;
    handler recreation also runs on the default test thread stack.
  - `handlers/invitation/enrollment_trust.rs` via private-field
    `RetainedEnrollmentVerifier` and `VerifiedEnrollmentResponse`; only the
    explicit setup-transfer path selects the expected verifier, and only
    successful expected-key verification constructs response evidence
  - `runtime/services/reconfiguration_manager.rs` via `ReconfigurationManager` and `SessionDelegationTransfer`
  - `runtime/session_ingress.rs` via `RuntimeSessionOwner`
  - `runtime/subsystems/vm_fragment.rs` via `VmFragmentRegistry`
- `CapabilityGated`
  - enrollment response recording in `runtime/services/ceremony_tracker.rs`
    and activation in `handlers/device_epoch_rotation.rs` require the opaque
    response witness; generic local or verified-ingress participant counts
    cannot accept the enrolling device
  - runtime reconfiguration admission in `runtime/services/reconfiguration_manager.rs`
  - session-owner capability checks in `runtime/session_ingress.rs`
  - runtime-facing readiness/lifecycle publication through sanctioned runtime coordinator paths

### Capability-Gated Points

- Runtime-owned readiness and lifecycle publication must flow through sanctioned coordinator APIs and capability checks rather than arbitrary handlers.
- Enrollment verifier restoration validates retained binding and policy before
  returning a handle. Durable response recovery must reverify the original
  response; deserializing a receipt cannot construct a response witness.
- Session and endpoint mutation must validate both current owner record and current owner capability.
- Runtime helper modules may stage work, but they may not author frontend- or harness-visible semantic truth without the owning capability.

Rules:

- Do not replace `MoveOwned` session/delegation transfer with an actor mailbox.
- Do not route long-lived mutable service ownership through move-owned handles.
- Do not author runtime-visible mutation/publication without the relevant capability gate.
- Public service APIs must take shared runtime-owned `TaskSupervisor` /
  `CeremonyRunner` roots from the runtime graph; they must not construct
  private owner trees internally.
- Actor-owned runtime services that expose command ingress plus owned background
  task groups must use one shared service-root abstraction for lifecycle state,
  supervised tasks, and command handles rather than re-implementing that owner
  shape per service.
- Service health must degrade structurally when maintenance obligations fail;
  loop-local logging is not a substitute for degraded lifecycle state.
- Inbound moderation and membership gating must fail closed when home state is
  unavailable, ambiguous, or missing authoritative roster membership; observed
  chat projection and current-home fallback may not authorize message
  admission.
- Runtime service APIs must not wait on generic "next reactive update" signals
  or return optimistic domain sketches when they claim to return a postcondition;
  converted chat/message/group queries and mutations, invitation-acceptance
  processing, and remote relational-fact pulls reduce committed facts and
  explicit cache/materialization work directly before returning.
- Converted runtime choreography start paths must surface typed start-failure
  reasons such as duplicate active session or stale task binding; caller retry
  policy must bind to that typed reason rather than parsing error strings.

### Verification Hooks

- `cargo check -p aura-agent`
- `just ci-actor-lifecycle`
- `just ci-move-semantics`
- `just ci-capability-boundaries`
- targeted runtime/service tests via `cargo test -p aura-agent`

Architecture/tooling split: runtime ownership boundaries that can be closed by types, visibility, or compile-fail tests should not rely primarily on shell grep checks. `just check-arch` remains the right gate for repo-wide runtime/integration invariants.

Legacy cleanup rule: `aura-agent` should not keep dormant runtime migration
infrastructure, compatibility constructors, or scheduler re-export shims once
no sanctioned caller remains. Shared transport simulation wiring is
`SharedTransport::new` plus explicit registration only; legacy one-off inbox
wrappers are removed instead of being kept as speculative escape hatches.

## Testing

### Strategy

Structured concurrency, ownership boundaries, and runtime composition are the primary concerns. Compile-fail tests in `tests/ui/` enforce type-level boundaries. Integration tests verify service lifecycle, session management, protocol choreography, and reactive scheduling.

### Commands

```
cargo test -p aura-agent
cargo test -p aura-agent --test compile_fail   # compile-fail boundary tests
```

### Coverage Matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Runtime missing required effect handler | InvariantRuntimeCompositionBoundary | `tests/ui/missing_*.rs` (6 compile-fail) | Covered |
| Private runtime internals accessible | InvariantCanonicalIngress | `tests/ui/*_private.rs` (4 compile-fail) | Covered |
| Protocol fragment registry leaked | InvariantSessionOwnership | `tests/ui/vm_fragment_registry_private.rs` | Covered |
| Service actor handle leaked | InvariantStructuredConcurrency | `tests/ui/service_actor_handle_private.rs` | Covered |
| Protocol-machine concurrent contract violated | InvariantStructuredConcurrency | `tests/telltale_machine_concurrent_contracts.rs` | Covered |
| Protocol-machine parity diverges across profiles | InvariantBridgeOwnershipAgent | `tests/telltale_machine_parity.rs` | Covered |
| Protocol-machine scenario contract fails | InvariantBridgeOwnershipAgent | `tests/telltale_machine_scenario_contracts.rs` | Covered |
| Production manifest fails admission | InvariantRuntimeCompositionBoundary | `tests/production_manifest_admission.rs` | Covered |
| FRP scheduler glitches | — | `tests/frp_glitch_freedom_test.rs` | Covered |
| Journal integration roundtrip | — | `tests/journal_integration_test.rs` | Covered |
| Session service lifecycle wrong | InvariantSessionOwnership | `tests/session_service_test.rs` | Covered |
| Auth service flow broken | — | `tests/auth_service_test.rs` | Covered |
| Recovery service flow broken | — | `tests/recovery_service_test.rs` | Covered |
| Threshold signing E2E fails | — | `tests/threshold_signing_e2e.rs` | Covered |
| Runtime bridge channel resolution wrong | InvariantBridgeOwnershipAgent | `tests/runtime_bridge_channel_resolution.rs` | Covered |
| Bootstrap preconditions not enforced | InvariantRuntimeCompositionBoundary | `tests/bootstrap_required.rs` | Covered |
| Production runtime accepts plaintext encrypted-storage policy | InvariantRuntimeCompositionBoundary | `src/runtime/effects.rs` (inline) | Covered |
| Reactive scheduler signals wrong | — | `tests/reactive_scheduler_signals_e2e.rs` | Covered |
| Reconfiguration integration broken | InvariantSessionOwnership | `tests/reconfiguration_integration.rs` | Covered |

## References

- [System Architecture](../../docs/001_system_architecture.md) — layer boundaries, guard chain
- [Authority and Identity](../../docs/102_authority_and_identity.md) — authority model
- [Effect System](../../docs/103_effect_system.md) — effect traits, composition constraints
- [Runtime](../../docs/104_runtime.md) — service actor patterns, concurrency profiles, link/delegate boundaries, async primitives
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) — canonical execution, admission guarantees
- [Formal Verification](../../docs/120_verification.md) — runtime parity lanes
- [Ownership Model](../../docs/122_ownership_model.md) — `Pure`, `MoveOwned`, `ActorOwned`, `Observed` categories
- [Testing Guide](../../docs/804_testing_guide.md) — required ownership tests, test strategy
- [System Internals Guide](../../docs/807_system_internals_guide.md) — instrumentation event families and required fields

Required AMP participant augmentation uses a channel-only persisted invitation reader. Storage listing/retrieval, record decoding, authoritative context validation, and required timestamp failures propagate their concrete sources. Corrupt records are decoded before type filtering; an unknown record cannot imply absent membership. Generic invitation `list_with_storage` remains an observed, best-effort API and must not establish canonical absence or readiness.

## Enrollment trust transfer boundary

Enrollment import admits only the opaque explicit app manifest pin. The runtime verifies the independently selected issuer key, actual local exported setup, exact signed baseline inventory, and package/policy bindings before cache, baseline, or key mutation. Immutable secure records reverify into non-Clone witnesses on recovery. A dedicated admission gate serializes reads/checks/writes within one effects owner; this is not a cross-process storage CAS guarantee. Issuance retains the exact expected signed manifest before publication; acceptance compares against that retained binding. Bootstrap ingress, historical response signing, generation registration, and activation are distinct owned permissions.

See [cryptography](../../docs/100_crypto.md), [operation ownership](../../docs/109_operation_categories.md), [shared user flows](../../docs/121_user_flow_harness.md), and [testing](../../docs/804_testing_guide.md).

### Enrollment activation recovery ownership

Enrollment registers its exact invitation and original bounded roster in secure storage before any finalizer or peer task starts. After signing bootstrap restores the active context, the runtime reconstructs pending registrations and validates their immutable ordered signing configuration and public package generation. Raw persisted responses are reverified under the original setup pin before process-local acceptance witnesses are minted.

An activation lease holds the ceremony decision gate across prepared tree mutation, signing generation activation and terminal settlement. The exact signed AddLeaf operation is durably prepared before application. Recovery checks canonical operation evidence rather than inferring commitment from a matching device leaf. Once preparation exists, cancellation, supersession and timeout cannot take activation ownership. The first secure terminal decision and final accepted inventory precede visible terminal publication.

The issuer reserves its invitation identity and stores the generation owner before writing packages. Generation ownership includes the ceremony, prestate, setup digest and ordered signer inventory. `EnrollmentGenerationReservation` is consumed only after the original registration and expected signed manifest are retained. Its opaque `RegisteredEnrollmentGeneration` carries the canonical invitation used by the initiator launch; callers cannot replace the artifact with another shape having the same IDs.

Failed generation retirement owns both the generation writer and ceremony decision lease, in that order. Required deletion errors retain their sources. The original failed decision survives deletion retries; configuration retains the cleanup inventory until all secrets are removed, and the activation profile is released last. An unissued allocation is retired only after committed invitation facts establish that its reserved invitation was not issued. If an invitation was committed before canonical registration persistence, bootstrap reconstructs that registration from its secure original snapshot and the committed canonical invitation rather than retiring its keys.

Response acceptance uses transcript domain `aura.invitation.device-enrollment-acceptance.v3` and the digest of the independently admitted signed manifest. The issuer loads the expected manifest from its own immutable secure record and revalidates its signature against the retained confirmation verifier; a peer-provided digest or a receipt's self-embedded verifier does not select trust. Legacy responses without a manifest binding remain decodable and cannot authorize activation.

These locks serialize one runtime's profile owners. They do not establish exclusive profile opening across concurrent operating-system processes. Successful multi-device participant acceptance and quorum enrollment remain separate integration requirements; unsigned participant responses are still rejected.

### Owned persistent profile construction

Production construction acquires the actual concrete infrastructure profile owner before secure-storage wrapping keys or persistent writers are assembled. Native synchronous factory acquisition and browser asynchronous preassembly converge on the same strongest private token. Runtime fields and actual secure-storage adapters retain ownership; exposed storage clones extend the lease until their own teardown. Custom trait guards cannot replace this token. Required profile errors retain native sources through builder and preset boundaries. Native shared keyring namespace ownership remains Unsupported rather than silently changing the configured secret backend. Browser historical plaintext secret namespaces fail before new signing state initializes. Canonical multi-record frontend profile handoff recovery remains a separate owner contract.

### Required issuer identity envelopes

Manifest issuance reads encrypted JSON participant-key envelopes from both sanctioned `signing_keys` and `participant_shares` layouts. Required reads validate bounded envelope version, authority, epoch, recipient and nonce, then authenticate the original wrap-key AAD before importing the plaintext package. They do not interpret encrypted JSON as a raw DAG-CBOR key or recover a corrupt selected record by trying another location. Contextual structural errors retain the original codec, storage or decryption cause and exact identity location. The actual bootstrap regression covers both layouts and corruption without granting historical response-signing authority. Observational legacy decoding remains separate.

### Terminal generation recovery regressions

Supersession candidates exclude every recorded terminal outcome, including cancellation and failure, so reissuance preserves the first decision. Exact retired-generation receipts permit idempotent observation after the corresponding generation profile was released; a matching receipt never authorizes deleting a later generation that reuses the epoch. Actual restart/reissue tests exercise both deletion interruption and unissued-allocation recovery. Enrollment negative-control fixtures use independent transferred manifests and real signed scoped controls; malformed peer decisions are tested after admission rather than with unpinned raw invitations.

### Descendant task ownership and drain

Each supervised tree has one admission lock and bounded registry (1,024 live groups, 4,096 active tasks, depth 64). Registry entries are weak; children retain their ancestors and running task guards retain their group, with no parent-to-child ownership cycle. Idle child entries are pruned, and the first structural task failure or panic is retained by every ancestor even after the child disappears. Cancellation closes the entire selected subtree under the same lock used for admission. Post-close or over-limit submissions drop their supplied future without polling, expose typed governance failures through health/drain, and return an already-cancelled owned handle.

Root and group active-task observations, bounded idle waits and forced aborts include all descendants. Task completion drops the owned future before removing its registration or waking ancestor drainers. Native abort uses the owned join handle; browser/local cancellation also has an abortable owner boundary. Abort requests retain active bookkeeping until the actual future is dropped, so requesting abort does not authorize releasing profile ownership or claiming idle. Shutdown success proves descendant drain; timeout/abort/failure remains a typed failed outcome, and synchronous shutdown is only a cancellation/abort request. Dropping a supervisor clone does not cancel a still-live supervisor owner.

Native/local regressions cover descendant IO causes and panic health, dropped child handles, resource-drop-before-idle ordering, shutdown/admission races using the real registry, limits and weak-entry pruning, and unpolled rejected futures. Browser execution of these contracts remains a separate platform gate; local Rust coverage does not claim browser runtime validation.

### Durable enrollment clock ownership

`EnrollmentWindow` is a runtime-private sealed execution owner. Registered issuer windows reuse their original tracked budget; invitee windows derive from independently admitted manifests and original secure admission time. Child windows retain the original parent checkpoint and execution lease. Required observation writes precede VM progression, verified response publication, and returning an operation result. Secure records bind the exact ceremony, device, generation or manifest digest, start, and deadline. Missing or invalid retained state fails closed; reimport cannot allocate a fresh window. The separate checkpoint write gate never re-enters the enrollment decision gate.

`CeremonyTracker` uses this original budget for timeout queries, activation eligibility, and cleanup. Cleanup returns the original typed failure and uses the fallible owned interval API. Task-group terminal failures, including timer errors, produce unhealthy service status and remain available through typed service outcome and shutdown errors.

Enforcement: strongest sealed attempt parameters, private constructors, `capability_boundary` declarations, checkpoint acknowledgment compile-fail coverage, and the Rust-native `async-session-ownership` lint. Its AST checks reject raw/no-op timeout executors, aliases, fresh budget reconstruction, and weaker attempt parameters in production enrollment. Explicit test-only model fixtures remain separate. Tests cover persisted highwater, sticky rollback, missing checkpoint, storage-failure source chains, postoperation publication failure, and cancellation before acknowledgment.

### Required native identity reads

The runtime bridge settings, device inventory, authority inventory, authentication, and nickname/MFA mutation contracts return `RuntimeBridgeError` and retain original tree, storage, and codec causes. Missing account configuration is an explicit optional read; required mutations reject it. Device inventory comes from canonical device leaves, and corrupt leaf metadata fails the required read. A current physical device absent from that tree is not inserted into the inventory. Settings contact counts and an uninitialized in-memory threshold snapshot remain presentation observations and do not establish signing readiness. Legacy account bootstrap and other diagnostic bridge contracts remain separate migration work.

### Enrollment window admission phases

The registered execution token is minted from actual tracker state and retained through checkpoint writes. The immutable `enrollment_window_ever_live_v1` binding precedes registry visibility and prevents deleted clock history from being recreated after live admission. A private pre-live capability can repair only an interrupted original allocation lacking canonical/live evidence. Exact secure allocation, deadline and shared observation/exhaustion identity remain required; checkpoints hold registry allocation continuity through their storage acknowledgment. Terminal outcomes for the same generation remain compatible with retaining historical clock evidence.

### Native account bootstrap failures

Account bootstrap existence and initialization contracts also return native structural errors. A required account read permits creation only on actual absence, preserves IO/codec causes, and validates an existing authority record before reporting initialization success. Authority-record creation precedes final account configuration publication; this sequencing does not assert a cross-record storage transaction.

### Invitee committed confirmation ownership

The invitee verifies the actual issuer-signed `Committed` control frame under
its independently retained initiator pin and exact admitted manifest. Verification
mints a private non-serializable confirmation witness; a boolean, invitation
status, supplied epoch or embedded verifier cannot replace it. Every successful
invitee VM exit requires this witness. The original admitted window owner
acknowledges and freezes its durable budget before the immutable secure
confirmation receipt is published.

Receipt recovery loads the original immutable transfer record, canonical
invitation and actual signed frame, checks the actual physical device and exact
manifest/ceremony/generation binding, and cryptographically revalidates them.
The historical clock path consumes only a secure-receipt-issued input and the
original acknowledged snapshot. It does not accept a caller-selected verification
time, allocate a fresh window, or authorize historical signing. Expiration after
a valid committed receipt does not retroactively revoke that receipt.

An immutable imported-generation owner is retained before import/key writes.
Generic effect and signing commits reject this owner. The sanctioned signing
activation consumes the durable reverified confirmation, matches actual retained
share/public package/canonical provisional configuration, and enforces monotone
persisted and in-memory epochs before publishing a signing context. A profile
WAL or serialized completion locator does not mint this capability. Revocation
and profile migration must continue through their current canonical owners;
retained ciphertext alone never proves current membership.

Required timeout checkpoints preserve storage and codec causes in `TimeoutBudgetError::CheckpointFailure`. This failure does not represent clock unavailability or elapsed time. Native and semantic classification follows its retained cause; unclassified checkpoint faults remain internal. Serialized diagnostics omit native error sources and grant no checkpoint authority.

### Required moderation evidence

Required moderation status returns native errors retaining actual journal/reactive causes. Ban/mute facts and reversals are strictly validated before status reduction; corrupt evidence cannot become an absent ban/mute. Canonical roster and journal authentication provenance remain owned by the existing runtime fact/materialization path, not inferred from returned booleans.

### Required enrollment task failures

The initiator choreography uses source-bearing one-shot task completion. Owned terminal settlement precedes task failure publication; the retained native failure keeps the original `AgentError` as its standard source and retains any secondary settlement error separately. Required descendant failures are observable through supervisor health and drain, rather than being converted to successful unit task completion.

Required enhanced-time scheduling propagates actual provider query and sleep failures. A failed timer does not constitute deadline expiration or successful wake-up. Native source traversal retains the concrete time-effect error through runtime scheduling and effect-system forwarding.

### Admitted enrollment import publication

The private `runtime::services::enrollment_import` owner requires the independently admitted manifest and immutable imported-generation binding before installation. Under the generation writer, it retains an original-tree digest before baseline replacement and immutably publishes exact issued share, public package, and provisional configuration. Legacy partial installations without this owner fail closed. Replay preserves an existing authenticated baseline prefix and later local operations. An existing committed-import receipt must reverify; committed replay never rewrites the adopted encrypted share or final configuration. These records coordinate publication and cannot authorize activation without the separately signed retained confirmation. Frontend profile handoff WAL, canonical current-membership/revocation checks, and quorum response ownership remain additional obligations.

### Enrollment confirmation membership snapshot

The committed-confirmation producer revalidates the actual local attested operation chain, exact activated epoch and retained participant configuration, and both device leaves before signing. Retained terminal outcomes and rendezvous key possession alone do not authorize a new confirmation after a known removal. The producer holds shared generation custody and the actual PersistentTreeHandler mutation gate through replay, membership checks and Ed25519 signing, excluding sanctioned signing transitions and concurrent local removal/import/replacement. This local custody is not a receiver freshness witness. Historical confirmation recovery verifies the originally admitted decision only; activation after later revocation still requires a fresh authenticated ordered membership checkpoint. Genuine quorum and non-root parent inventory remain required separate evidence.

### Signing material and tree decision lock order

The shared enrollment generation gate precedes service signing transitions and tree decision custody. Generic bootstrap, rotation and rollback acquire generation custody before changing persisted or in-memory material; verified activation already requires the generation owner. Enrollment control signing retains generation custody and the actual tree decision lease through verification and signing. Read-only retained candidate validation grants no transition authority. Local generation/tree custody does not establish receiver freshness or remote no-later-revocation evidence.

### Shared preassembly profile resource

`EffectSystemBuilder::with_profile_owner` accepts only the concrete `aura-effects` profile lease resource. Bootstrap and subsequent runtime assembly may retain clones of that same physical owner; construction validates the selected profile before provider or wrapping-key mutation. Testing and simulation builders reject a production lease. Boxed core leases or caller-provided no-op guards cannot manufacture this provider guarantee. Frontend startup must actually transfer its original owner; this builder contract alone does not establish frontend WAL recovery or current membership authorization.

### Authenticated enrollment committed transition

Version-two enrollment control signatures bind the exact attested operation suffix extending the independently admitted original baseline. Receiver verification derives each parent key and policy from that staged authenticated tree, verifies every transition, and requires both physical devices at the exact pending epoch before minting confirmation evidence. Legacy frames lacking the suffix fail closed. The resulting sealed confirmation retains its authenticated committed tree for owned activation. This proves the original committed checkpoint; later revocation and restart freshness still require independently ordered current checkpoint evidence. Peer-supplied key inventories and raw committed IDs cannot establish this authority.

### Native VM bridge round failures

Host-bridged rounds retain native step, transport-send and receive failures in
`AuraVmBridgeRoundError`; session ingress forwards them through the source-bearing
`BridgeRound` variant. Missing peer mappings remain a distinct configuration
failure. Owners classify transient retry from these native causes, retaining the
original admitted timeout window and pending-message ownership. Display messages
and serialized diagnostics cannot supply retry authorization.

The concrete VM queue retains every frame until its exclusive send lease
acknowledges delivery. The lease crosses awaits without holding a blocking lock.
Cancellation during delivery marks the retained frame unknown and prevents
replay; native definitely-unsent errors preserve pending order and retry
eligibility. Tests cover concurrent owner rejection, enqueue during delivery,
partial batch progression, and dropping an awaited in-flight future.

### Enrollment refusal and failed readout

The enrollment response verifier retains the independently pinned setup signer
and canonical issued invitation. Its opaque rejection capability is distinct
from acceptance evidence. The ceremony tracker consumes that capability under
the original window and terminal decision gate before signing a failed control.
The invitee's immutable failure receipt is separate from confirmation/adoption
receipts and revalidates the pinned control plus frozen original-window ACK.
Observed invitation status cannot substitute for either proof.

Regression coverage includes real threshold-signed acceptance/refusal domain
confusion, tampered device bindings, first-terminal-decision conflicts, and two
actual connected runtimes exchanging refusal and signed failure without
adoption. Native VM request retries require the exact transport failure and
original subject destination. VM teardown errors retain both execution and
close causes, and prohibit retry when cleanup itself fails.

### Imported committed verification keys

An imported committed transition verifies in a private view populated from independently pinned manifest node/epoch verifiers. Verified same-epoch operations retain that key lineage; the exact pending root epoch uses only the public package and canonical provisional policy bound by the independently admitted manifest and invitation. The configured cryptographic threshold must meet the actual tree policy minimum. Conflicting node/epoch pins or unavailable branch keys fail closed. This view never substitutes ambient provisional identity keys or assigns a root package to arbitrary branches, and a historical receipt does not establish absence of later revocation.

### Confirmed enrollment profile publication

The profile handoff owner consumes the independently reverified durable Committed capability. It publishes an immutable prepared descriptor before key activation, an immutable committed descriptor after activation, and then the derived account projection. The descriptor binds the exact physical device, provisional authority, subject, invitation, ceremony, epoch, and manifest digest. Prepared bytes locate and coordinate the original proof; they never authorize subject identity by themselves. Account settings read/modify/write share the runtime profile gate, and production adapters retain their cross-process profile lease. Retrying the same proof preserves committed metadata. Restart identity selection and native/browser lease transfer require a separate preassembly owner that validates original secure evidence; this publication sequence alone does not provide that integration or establish absence of unknown later revocation.

### Required app task supervision

The runtime `TaskSupervisor` admits required native and local futures through checked registration. Rejected admission retains the structural governance cause and drops the unpolled supplied future before returning. An admitted required failure retains its concrete `AuraError` source in root supervision and makes idle/drain fail. The unit-task compatibility surface is unsuitable for required refresh ownership.

### Timeout selection and VM teardown

Enrollment attempts retain their actual VM handle in an owner slot outside the
timed future. The operation borrows the slot; timer selection can cancel its
work without dropping the handle required for asynchronous close. Required
close runs before retry or result return and preserves execution and close
causes. An actual-runtime regression selects timeout while the operation
borrows the VM, then verifies owner retirement after close.

Required VM close retains `AuraChoreoEngineError`; required runtime close retains
`ChoreographyError` through the standard native error source chain. Forced drop
retains the first concrete close or retirement failure, with a typed secondary
owner failure available through the composite cause. This does not recover
structured detail already represented as text by the upstream session lifecycle
provider. Required source preservation does not turn forced drop into an
asynchronously acknowledged terminal proof.

### First enrollment decision publication

The retained setup verifier, response receipt, pending signing-generation binding, pending registration, and issued manifest publish through actual immutable secure-provider admission. Owner APIs retain their sealed setup/generation/acceptance/manifest inputs. Existing records are bounded and compared against exact bindings; receipt and manifest reuse reverify original cryptographic evidence and canonical decision rather than requiring identical randomized signature bytes. Mutable secure-store capability flags do not authorize enrollment decisions.

Generic secure-store replacement remains an infrastructure escape surface until provider write protection is enforced for previously immutable locations. Atomic first publication alone does not prove lifetime immutability or authorize cleanup.

### Held generation registration admission

Live pending-registration publication requires the actual held `EnrollmentGenerationReservation`. The private constructor retains its physical effect owner and generation gate. Admission checks the exact effect instance, issuer/invitation/ceremony/prestate/epoch/setup binding, physical participant roster and threshold, and initial nonterminal state. It preserves the original immutable allocated start, timeout and nickname and permits only budget checkpoint continuation. Structural binding failures retain typed `HeldEnrollmentRegistrationError` sources.

Only verified recovery reaches the private storage helper without the live reservation, after rebinding the original allocation, retained setup and signed issued manifest. Required actual-owner negatives reject another effect instance and a previous ceremony's actual state. The typed API rejects callers supplying only registration facts; generic tracker allocation and provider mutation surfaces still require their own ownership enforcement.

### Initial persistent enrollment allocation

Generic `CeremonyRunner::start` and tracker registration cannot allocate persistent device enrollment from plain request facts. The owned enrollment entry accepts the actual held `EnrollmentGenerationReservation`, and immutable allocation publication independently checks that owner before writing. Another effect instance, physical roster/policy, ceremony, epoch, prestate, or terminal state cannot reuse the reservation. Missing ownership returns structural `RequiredOwner` before durable or tracker mutation.

Guardian ceremonies retain their generic registration contract. Nonpersistent tracker models may register enrollment facts for pure state-machine testing; they do not authorize runtime signing activation. Recovery does not re-enter generic registration: it reconstructs original verified durable evidence and preserves original start, deadline and observation history. Declaration attributes point to the typed owner delegates and validation methods, with actual raw-registration refusal tests providing the integration guard.

### Enrollment tree epoch activation fence

The original activation capability owns a version-2 immutable prepared bundle containing the exact admitted prestate/baseline, signed AddLeaf, and original-parent-key-attested RotateEpoch fence. Both signatures are verified against actual retained parent packages before the complete history is published. The actual tree decision lease remains held through cryptographic key activation and terminal commit; copied ids or prepared bytes cannot replace that custody. Recovery reuses the exact saved signatures, preserves later local evidence, and rejects divergence, missing membership, or a different pending epoch. Version-1 preparation with no signed fence decodes for diagnosis but fails closed. AddLeaf alone never establishes a new tree epoch. Multi-device signed commit transport must separately carry/authenticate the exact fence; the legacy leaf-only commit is insufficient for that path.

Sender cancellation reads the required persisted sender record and genuine
retained issuance. Enrollment cancellation wins the original-window terminal
CAS before publishing `InvitationCancelled`; a rejected or committed first
decision cannot be overwritten by a later cancellation. Its move-only negative
capability authorizes only the matching local cancellation publication, never
membership or activation. Required storage, codec, capability, flow-budget and
clock failures preserve their concrete source; observed cache absence cannot
authorize this path. Updating the regular cancellation status does not replace
the separately retained enrollment secret payload.

Leaf-only bootstrap histories do not materialize a branch policy or signing-key node. Enrollment verification projects exact node/epoch keys and threshold cardinalities exclusively from the independently admitted signed parent inventory or exact signed pending package policy. The projection satisfies the normal attested-operation verifier without inserting a canonical branch into the journal. Any branch policy already materialized by authenticated operations remains a minimum, and absent nonroot verifier inventory fails closed. A pending root requires authenticated leaves actually attached to node zero, rather than an invented root.

### Original allocation recovery custody

A pre-live device enrollment can reacquire generation custody only from its retained ceremony selector under the actual generation gate. Recovery validates the protected original allocation, independently retained setup verifier, exact pending signing package/configuration digests, physical profile identity, and active epoch before minting a held reservation. An immutable generation-allocation record independently binds the original reserved invitation and ordered signing roster; mutable lifecycle profiles cannot replace that evidence. Caller snapshots cannot supply registration fields or restart the timeout. Admission observes and durably checkpoints the original deadline/high-water budget before exposing a live tracker entry; terminal originals cannot be reopened. Initial mutable clock/profile publication is atomic and preserves an existing record. Retirement and terminal first decisions are immutable evidence and must be reread and checked before subsequent release/publication.

Cancellation preparation consumes required sender-record custody and borrows
retained issued control; observed invitation values cannot substitute. Both
retain the exact native runtime owner. Guard-issued preparation and the
tracker-issued negative terminal token must agree with the publication runtime,
including when authority and physical device identifiers are equal. A different
runtime cannot sign through borrowed control or publish its cancellation.
The required regular sender-record decoder rejects records larger than 1 MiB
before decoding; oversized and malformed data remain distinct typed failures.

### Enrollment signing roster and authenticated topology

Enrollment keeps the original independently pinned signing quorum separate from the authenticated tree child topology. A signed AddLeaf changes observed child edges before the original signing key attests the RotateEpoch activation fence. Verification retains the exact signed quorum minimum and signer-roster upper bound while deriving topology cardinality solely from authenticated branch and leaf-parent edges. A verification projection does not materialize a canonical branch. A policy already materialized by authenticated tree operations remains an independent minimum; invalid or incompatible canonical policy metadata fails closed and requires an authenticated policy transition rather than a local repair. Missing node-specific verifier inventory cannot be replaced with the epoch root package.

Required sender enrollment custody combines the bounded regular record with its
separately retained secure original payload. Secure reads, decoding and exact
immutable metadata comparison must succeed before custody is minted; a redacted
record alone cannot substitute. Only regular lifecycle status overlays the
retained original. The regular decoder is bounded to 1 MiB and the retained
secret decoder to 4 MiB. Cancellation status publication redacts the hydrated
payload again and preserves the secure original bytes.

### Authenticated enrollment rotation roster

`AuthenticatedEnrollmentRotationPlan` is move-only and non-deserializable. It
holds actual generation then tree decision custody while checking attested
history, active epoch, configuration, issuer membership and invitee absence.
Rotation consumes the plan; raw participant vectors and prestate hashes cannot
authorize key generation. The reservation retains both guards through original
registration. Original orphan cleanup has distinct negative custody; live
recovery additionally validates the current roster and the original window.
Same-original issuance continuation and full multi-device/nonroot coverage
remain required work.

Choreography receives own a fixed local `TimeoutBudget` instead of registering
timer handles. Notification wakes retain that deadline; dropping the receive
future leaves no timer registry entry. Required read/sleep failures remain typed,
and successful sleep alone cannot establish deadline expiry.

### Physical bootstrap signer representation

New single-device bootstrap signing contexts retain the actual physical Device
participant that the authenticated genesis leaf names. Solo signing and local
key agreement select the participant from the authoritative retained signing
context; they do not substitute a guardian label for a device. Legacy guardian
contexts remain distinct and cannot establish an enrollment device roster.
Historical conversion requires original genesis, device and key evidence.
Required envelope reads retain their original wrapping key and cannot create a
replacement when it is missing. Both initial wrapping-key producers use atomic
immutable publication and adopt only the exact retained winner.

### Registered task execution identity

`task_registry` mints move-only registration custody only after actual bounded
registry admission. Its private future wrapper installs an opaque owned identity
for each poll and destructor, restoring the caller lexically before returning to
the executor. Identity combines actual task-tree custody, group and task, so
separate groups cannot collide on their local counters. It is stable across
awaits, executor thread migration and native/browser local execution.

Choreography session binding prefers this actual registered identity over
executor or thread fallback. Browser siblings therefore do not share one thread
owner. Observing or cloning an identity cannot enter a scope, admit a task, or
construct registration custody. The fallback remains for direct callers outside
runtime supervision; it is not used for service-owned sibling tasks. Tests cover
real native/local registry execution, the browser-local sibling state contract,
nested polls, panics, cancellation and drop before first poll. These ownership
laws do not establish authenticated enrollment protocol completion by themselves.

### Forced VM owner retirement

`SessionOwnerCapability` is an opaque token issued only by the choreography
claim/transfer owner. A private per-claim identity distinguishes real owners
even when separate runtimes use identical session IDs, labels and counters.
Metadata getters expose observations; the actual claim remains required for
validation, transfer and retirement. External constructor, field-mutation and
wire-deserialization compile-fail tests cover reconstruction; the equal-metadata
two-registry regression proves rejected mutation leaves both real owners intact.

A dropped move-owned VM session retires its exact registered choreography owner
and local fragment custody synchronously. Owner generation and scope validation
share the ownership-transfer lock order, so stale handles cannot retire a newer
owner. This resource retirement supplies no signed terminal decision and performs
no asynchronous close or physical-time observation. Ordinary paths retain the
required explicit close contract. Unknown transport delivery remains unknown;
retirement neither acknowledges nor replays pending frames.

Registered task destructors retain concrete retirement failures in the existing
service task health owner before cancellation completion and idle publication.
Task poll identity scopes remain lexical during destructors.

### Enrollment negative terminal notification

Authenticated enrollment cancellation uses the separately admitted finite
`aura.invitation.device_enrollment_terminal_notice` protocol. Its session identity
is domain separated from the request/response session and binds the original
invitation and admitted manifest digest. A notification never advances the
request/response VM's program counter. The wire envelope is untrusted: only the
independently pinned issuer signature and exact original manifest, invitation,
ceremony and physical-device bindings issue negative terminal evidence.

The issuer sends only after the durable cancelled terminal decision. The
receiver's listener belongs to the existing invitation service task tree and
shares the original admitted window's lease, checkpoint and observations. It
retains a distinct immutable signed failure receipt before reporting failure.
Negative evidence cannot authorize device adoption or committed recovery.
Cancellation and ordinary response races close their actual owned session slots;
forced drop retires exact session custody without claiming asynchronous close
or terminal evidence. Required failure causes remain in native observation and
service task health. Notification retries retain the original deadline and
only reconcile definitely unsent, exactly scoped transport failures.

### Original enrollment reservation and identity context

The enrollment reservation owner retains the original invitation ID, subject, physical issuer device and creation timestamp as bounded immutable first-decision evidence before any pending signing packages are generated. The live reservation privately retains its actual effect-system owner; equal authority/device identifiers in another runtime cannot authorize retention, rotation or invitation preparation. Held rotation requires that exact retained reservation. Repeated publication observes the original clock anchor rather than renewing invitation eligibility. This reservation identifies the original operation; it cannot grant setup trust or authorize registration by itself. Same-original continuation additionally requires independently retained setup, protected original allocation/window, exact signing generation and a fresh held authenticated roster decision. Missing original evidence fails closed.

Required issuer identity readers accept a sealed local signing context and read one exact epoch and physical participant. Fresh issuance uses actual required active policy and passes the same context into manifest export. Later control signing selects the original epoch only from its locally retained issued-manifest owner and checks its original verifier; current membership remains a separate held decision. Arbitrary current/one/zero epoch search and envelope-selected participant identity are forbidden. Canonical single-signer participant-share presence commits the required read to that row; explicit absence alone permits its exact same-context historical solo companion. Legacy Guardian-labelled bootstrap migration still requires original authenticated creation/key evidence and crash-safe conversion; unproved aliases do not pass physical roster admission.

Declaration guards reject adding `Clone` or `Deserialize` to live issuance
reservations. Actual runtime fixtures check equal-ID foreign-owner rejection
before protected reservation publication and preserve the original clock anchor
on repeated publication.

### Exact notice identity and bounded lexical delegates

The original registered enrollment window retains a single sealed notice
identity from the actual retained issued control. It binds the canonical
manifest transcript and digest, including signed expiry, to the original
runtime and registered generation. Clones use the same binding; rebinding to a
different digest, transcript or expiry fails closed. Admitted notice guards
match the retained original expiry explicitly. No serialized value constructs
this runtime-local binding.

Enrollment facade, attempt and VM progression delegates use bounded heap
allocation while retaining the same lexical owner, references and time window.
Caller future size is constrained independently of executor stack settings.

### Signed notice validity attenuation

Issuer notice execution is bounded by both the original registered ceremony
interval and signed manifest validity. An original owner derives its child
endpoint from one acknowledged physical observation; a later read cannot shift
that endpoint forward. The child shares original lease, observations and
checkpoint, and never renews the registration. An already elapsed manifest
validity is a retained typed domain expiry, not an invented clock failure or
retryable transport diagnosis. Active cancellation checks this signed validity
before the durable terminal CAS. Already-cancelled idempotent local replay
retains the first decision without authorizing a new expired notification.

### Lexical invitation command dispatch

Invitation effect command dispatch allocates the selected typed dispatch future
before the command loop or its timeout facade embeds it. The same lexical owner
retains the command, authority/effect references and mutable pending receipt.
This does not add a task, change required versus best-effort command policy, or
allocate a replacement eligibility window. A type-only caller frame guard and
the actual default-stack enrollment flow cover this boundary.

### Lexical reserved invitation preparation

The device-enrollment invitation factory moves its actual reserved issuance and
secret payload into a heap allocated delegated future before returning to its
caller. Its private body retains the required reservation-backed handler path
and deferred delivery owner. No separate task, profile owner or fresh eligibility
window is created for allocation. Default-stack execution remains the required
integration check beyond caller frame measurement.

### Lexical signed manifest preparation

The owned enrollment manifest exporter allocates its delegated future before
returning to the initiation caller. The original reservation, independently
selected setup and exact signer context remain borrowed by that same lexical
task. Cancellation drops the delegated owner without creating another task,
renewing its window or resolving signer identity from weaker IDs. The actual
enrollment caller runs on the default test stack; successful unpolled frame
measurement alone does not establish execution safety.

### Lexical runtime assembly allocation

`EffectSystemBuilder::build` allocates its private delegated future before
returning it to an async caller. The delegated future retains the same selected
profile lease and context borrow; it has no independent task or supervisor.
Dropping the caller drops that construction owner. The unpolled enrollment frame
regression measures the production builder and its actual fixture caller against
a 16 KiB frame budget, while the actual enrollment caller also runs on the
default test stack. Frame size coverage does not establish successful runtime
execution or protocol completion.

### Required active signing material

The runtime effect signer selects its epoch, policy, participant, and public
package from required retained state under signing-generation custody. Missing
or malformed state is a source-bearing failure. A solo signature additionally
requires agreement between the retained public package and the public key
derived by the cryptographic effect from the selected private package. A local
share does not establish quorum; multi-party signing requires its owned
threshold-agreement producer. Historical enrollment confirmation keys remain
selected only by the retained original issuance witness.

### Required fact commit causes

Canonical fact commit retains original order-clock, serialization, storage and
publication failures through the native error chain. Shared handler adapters
retain that `AuraError` instead of replacing it with an effect error string.
Native diagnostics can therefore differ from earlier flat effect messages;
message text is not a policy discriminator. A closed fact sink is a required
publication failure even if journal persistence or an enrollment terminal CAS
already succeeded. That partial outcome does not report full publication
success, overwrite the first terminal decision or renew the original window.

### Original cancellation preparation window

Active cancellation preparation retains a tracker-issued observation capability
for the original shared clock, expiry state and checkpoint allocation. It does
not acquire the initiator's execution semaphore. Guard preparation, first-decision
CAS and local publication are bounded by the original physical window attenuated
to signed manifest validity. Their required observations acknowledge the same
durable original checkpoint, and replacing an allocation or observation owner
fails closed. The preparation capability cannot admit a VM session.

An already persisted Cancelled decision yields a distinct negative publication
capability. It cannot reopen execution or renew eligibility. Initial selector
lookup and historical negative publication require their own bounded ingress
scope; that local bound is not enrollment admission authority.

### Ceremony cancellation ingress

Device-enrollment runtime cancellation IDs select the protected original issued
manifest. A bounded required reader verifies that issuer artifact, hydrates its
exact sender record, and retains its independent setup-bound control through the
original-window cancellation CAS. The selector alone cannot authorize terminal
publication. The bridge does not convert cancellation into a generic runtime
failure or delete the pending signer before its signed notification owner runs.
Guardian rollback uses its separate decision owner.

### Proved legacy physical bootstrap migration

Epoch-zero Guardian encoding is converted to the actual physical Device only
under generation, signing-transition, and tree custody. The migration verifies
the original retained secret/public package, authenticated creation operation,
and durable creation index. It rewraps the same secret and records an immutable
original decision before publishing canonical Device policy. The policy carries
that decision digest; required effect reads, service restore, and live solo
reads revalidate it. Missing origin evidence fails without selecting a fresh
bootstrap or regenerating keys. Other epochs, mixed rosters, and quorum contexts
are outside this migration.

Enforcement includes real encrypted historical-layout migration, idempotent
service restart, protected-decision physical loss, and durable creation-index
loss regressions. Metadata omission preserves fresh canonical encoding.

### Required startup authorization hydration

Persisted Biscuit hydration returns a required result consumed by runtime
assembly. Only exact typed secure-record absence permits the fresh bootstrap
path. Existing records are bounded and cryptographically verified before cache
publication; malformed record and verification failures retain typed causes.
The runtime builder retains the native hydration cause through its initialization
error. Regression coverage uses actual profile storage and token production to
separate absence, truncation, valid restoration and corrupted token rejection.

Production authorization-cache publication is private to runtime assembly and
bootstrap. Malformed observed-cache fixtures use a test-only crate-local setter.
The external `observed_cache_cannot_publish` compile-fail case is part of the
agent UI guard inventory and prevents restoring a public mutation escape hatch.

### Enrollment generation and decision custody

`EnrollmentGenerationCustodyCapability` is minted only by the effect system's
actual generation gate. `EnrollmentGenerationDecisionCapability` combines it
with the actual tracker decision guard and verifies both runtime owners. Live
roster plans, generation reservations and activation capabilities retain this
composite before taking a tree lease. Owned registration and supersession reuse
the reservation's tracker decision; they cannot introduce a recursive lock.
Verified signing activation checks the activation's physical generation owner.
Negative orphan cleanup remains a separate capability without live publication
rights. Private fields, declaration attributes, compile-fail guards and actual
runtime contention tests enforce this boundary. These guarantees do not imply
remote freshness or a distributed quorum producer.

Required reserved invitation creation reads physical time, verified Biscuit
frontier and flow budget without defaulting provider failures. Missing frontier
remains an actual missing capability; evaluator and journal failures retain their
concrete sources. Canonical original creation/expiry survive resumption while
current capability evaluation remains current. This prerequisite does not prove
same-original issuance continuation or distributed charge/send atomicity.

Completed enrollment registration evidence retains a borrowed reference to the
actual physical effect owner after releasing generation/decision/tree custody.
The issuer service must validate this reference before starting protocol or
delivery tasks. Equal authority/device identifiers from a distinct runtime do
not authorize handoff; an actual two-runtime regression enforces this boundary.

### Required targeted VM retirement

Required VM retirement disposes only the owned target session, including live or
blocked coroutines, scheduling eligibility, handoffs and scoped communication
and resource state. Unrelated sessions remain usable. Host metadata removal
follows the actual backend acknowledgment. The cooperative backend retains
stable coroutine IDs through its existing index rebuild; the threaded backend
validates and rebuilds its explicit ID index after removal.

Threaded worker execution is joined by the engine's synchronous worker scope
before exclusive disposal begins. Acknowledgment means no target job remains;
it does not require destroying the shared pool or other sessions. Naturally
terminal session status and epoch are preserved, and repeated disposal returns
its original compact acknowledgment. New close epoch advancement is checked
only for a genuinely active target.

Concrete backend failure remains in the standard native source chain. Required
closure cannot succeed by parsing or suppressing an unsupported error, merely
removing host metadata, or disposing an entire engine containing other sessions.
Historical compact summaries and diagnostic traces follow the dependency's
explicit archive policy. Shared global guard state is not claimed as exclusively
owned by the target session. VM disposal is local cleanup and does not establish
remote delivery, a signed terminal decision or protocol completion.

### Final enrollment verifier capture

EnrollmentFinalVerifierInventoryCapability borrows the actual held EnrollmentGenerationReservation and is neither Clone nor deserializable. Capture checks the exact physical history, current protected epoch/configuration/public package and authenticated device roster. Export requires the capture and exact original invitation/ceremony/setup/pending binding. Historical parent tuples cannot select the active final epoch. Root-only physical package storage currently rejects nonroot histories; exact-node package ownership/persistence remains a required implementation task, not a duplicated-root fallback.

### Registered enrollment execution admission

The original registered execution window is admitted before any initiator task
is spawned. Its actual semaphore lease and persisted clock owner move into the
initiator. A second facade sharing the same runtime observes structural
`AlreadyRunning` and does not spawn another initiator, sole finalizer, or peer
rotation owner. Closed lease, original checkpoint, and clock faults retain their
sources and remain failures. Task admission faults retain supervisor evidence;
an already-owned window never authorizes ceremony failure or retirement.

The runtime identity and immutable original registration remain prerequisites.
The lease bounds concurrent execution; it does not prove remote acceptance,
new quorum agreement, or completed profile activation. Recovered runtimes must
reauthorize the exact original window before claiming a new execution lease.

### Original enrollment history identity

A fresh authenticated roster plan seals the canonical baseline count and digest
into the protected original generation allocation. An unissued continuation
requires that exact prefix of the still-authenticated current history under the
held generation, tracker decision, and tree custody. Equal reduced state and
ordered physical membership alone cannot authorize a different original
manifest. The mutable generation profile must equal the original protected
allocation before it can be resumed as unregistered custody.

Older records may decode with missing baseline identity for observation or
negative reconciliation, but cannot authorize renewed live issuance. Missing or
diverged original history is a typed refusal; a new fingerprint is never repaired
from the current snapshot. This prerequisite does not complete original issuance
continuation, which additionally requires the original setup, reservation,
window, signed manifest, retained material, and registration-before-delivery.

The admission boundary requires the actual registered-generation capsule and
retains its actual tracker owner. It revalidates the exact immutable canonical
registration under the decision gate before leasing its original execution
window. Equal physical IDs or a second tracker sharing effects are insufficient.

### Cancelled enrollment notification recovery

Post-bootstrap recovery distinguishes a retained Cancelled first decision from an
active enrollment registration. Pending generation secrets remain retired. A
separate move-owned notice capability binds the independently retained issued
manifest, actual runtime, immutable first decision, original clock checkpoint and
original execution semaphore. It authorizes only the finite signed terminal-notice
protocol; it grants no Request, Accept, Confirm, activation or membership authority.

Notification eligibility ends at the earlier of the original registered deadline
and the signed manifest expiry. Restoring a terminal decision does not renew either
bound. Required observation and checkpoint acknowledgment precede signing and
continuation. Signing uses the currently owned issuer identity only while its key
still matches the independently retained issuer verifier. Known original or signed expiry completes the finite recovery owner with a typed
eligibility-ended disposition and prevents further sends. Required clock,
checkpoint, signer, transport and teardown faults retain their original typed
cause through task failure and drain. Both preserve the Cancelled decision.

Failed enrollment retirement acknowledges its exact immutable receipt after
required secret cleanup and retains the immutable original allocation record.
Restart must not attempt generic deletion of that immutable evidence or immutable
wrapping secrets; an explicitly mutable pending slot has a distinct lifetime. Distinct allocation
history and live pending-slot ownership must also prevent a later same-epoch
ceremony from overwriting or reusing an old allocation grant.

Successful public cancellation and retries preserve retained issued control
through primary negative publication, then admit the same finite original-window
notice owner used during bootstrap recovery. No raw ceremony re-resolution is
needed. An existing execution lease is a typed duplicate disposition. Required
subsidiary preparation/admission failures remain in the task group's health and
drain without rewriting the primary Cancelled result or spawning diagnostics.

### Confirmed imported public parent archive

The invitee activation owner publishes immutable public parent inventory before
exposing its imported signing context. The archive is bound to the actual physical
device, original provisional authority, subject, invitation, independent manifest
digest, and exact verified committed history. Its loader revalidates the original
secure admission, frozen acknowledged clock and issuer committed proof; decoded
archive bytes cannot mint evidence. The resulting capability borrows the actual
effect system and imported history collection requires that exact capability.
Historical tuples come from the pinned signed manifest. Only its exact signed
pending public generation may extend that inventory, after canonical provisional
policy and package digest checks. Missing native metadata does not trigger archive
selection. Nonroot generations and later unsigned inventories remain rejected.

The connected real-confirmation fixture exercises durable archive reload, foreign
runtime rejection and substituted history rejection. This coverage does not prove
process restart/profile handoff: the native/browser WAL and runtime reconstruction
must still preserve and recover the original provider and confirmed receipt.

The public archive v2 additionally retains the exact pending root verifier from
the original confirmed invitation's signed package/policy commitments and verified
committed head. Imported parent collection consumes this explicit source during
activation, before exposing signing context; it does not require a retired private
share or read a missing epoch package into a different layout. Only already verified
same-epoch extension heads can reuse that exact root policy/package. A later epoch
or another signing node needs additional independently authenticated evidence.

Immutable v1 archive bytes remain audit evidence. They are never rewritten or
implicitly promoted. Explicit publication from the reverified original confirmed
receipt creates v2 under its separate namespace. The real confirmation fixture
encodes the historical v1 shape, verifies no automatic v2 admission, and checks
that explicit v2 publication leaves original v1 bytes unchanged.

### Enrollment generation history and mutable slot ownership

`read_owned_enrollment_generation_profile` validates the bounded canonical live
slot against immutable original allocation bytes and the immutable registered
first-decision seal. `complete_registration` acknowledges that seal before
updating the mutable phase. A slot phase downgrade therefore cannot reopen a
registered allocation. All generic activation fences include explicitly proved
legacy records until an owned migration validates their original history.

Legacy migration consumes the actual generation-custody capability. Registered
legacy records additionally require the existing protected registration binding;
no historical record is overwritten or implicitly admitted on a read failure.
Pinned and orphan retirement release only the mutable live slot, after exact
completed cleanup evidence. Interrupted orphan cleanup restores a separate
negative-only capability from its original first decision and original allocation;
missing partially deleted material cannot mint a live reservation.

The encoded-layout fixture checks protected legacy migration and mutable-slot
corruption. The real same-epoch reissue fixture retains the original public setup
and issuer runtime, checks immutable history preservation, and rejects mutable
registration and roster forgery. Its retirement success requires the provider's
closed immutable-secret retirement API; the slot split alone supplies no
cryptographic erasure or generation-specific wrapping-key lifetime.

### Admitted clock anchor and checkpoint boundary

`EnrollmentWindowCapability::admitted` retains the actual
`AdmittedEnrollmentWindowLeaseCapability` through original publication recovery.
The lease checks physical effect-owner identity. Recovery rereads and revalidates
the original protected admission before completing missing initial bytes. Ordinary
required/reimport readers never invoke this completion producer.

`admitted_enrollment_clock_anchor_v2` retains the immutable original binding and
interval; `admitted_enrollment_clock_checkpoint_v2` is the independently mutable
highwater record. `admitted_enrollment_clock_ever_live_v2` acknowledges the first
execution admission before checkpoint-backed execution. After that decision, loss
of either required clock record cannot cause initialization. Legacy v1 migration
under the same held owner retains its original capped interval and bytes.

The actual public enrollment fixture exercises checkpoint updates against the
protected anchor, interrupted initial publication, restoration without deadline
renewal and missing checkpoint after live admission. The interval regression
distinguishes original legacy attenuation from the fresh signed interval. Actual
same-profile AgentBuilder reconstruction and legacy-layout provider-fault cases
remain required broader integration validation; these checks alone do not prove
full restart closure.

### Protected enrollment response policy

`EnrollmentResponsePolicy` is stored in the original protected generation before
allocation. Its private constructor selects the established remote response
policy from the fresh authenticated signing plan: nonissuer responders form the
response roster, with a required count bounded by that roster. The held generation
reservation exposes this original commitment to the actual issuer registration
producer; recovered registration never computes it from weaker response fields.

Required allocation and registration compare the response threshold/count
exactly, independently of the full key signing threshold/ordered roster.
`HeldEnrollmentRegistrationError::ResponsePolicyBinding` retains expected and
observed values structurally. Missing historical response commitments fail as
`ResponsePolicyMissing`; proving and publishing a separate historical commitment
is remaining migration scope. Pure tests exercise distinct signing/response
policy and source-bearing mismatches; actual public fixture registration remains
the integration gate. No response policy grants a distributed signing quorum.

### Proved old response-policy supplementation

`StoredLegacyEnrollmentResponsePolicy` binds the exact protected old allocation
and allocated-registration digests. The migration producer consumes actual
`EnrollmentGenerationCustodyCapability`, validates original setup and responder
bindings, and retains the original registration's exact required/count values.
There is no recovered-threshold clamp and no new start or deadline.

Required slot and recovery readers revalidate the immutable supplement. A private
serde-skipped cache carries the reverified policy into synchronous held-generation
checks; serialized allocation bytes remain the historical shape. Lost or changed
proof cannot restore that cache. The truly old schema fixture removes the new
response field from the encoded historical owner while retaining genuine
protected original registration. It checks explicit migration, unchanged original
bytes/deadline, cache non-restoration through serde and actual provider proof loss.

This fixture exercises protected historical schema under a real runtime, not full
AgentBuilder profile reconstruction. Legacy clock/profile reconstruction and
original eligible unissued continuation remain required integration scope. Old
allocations missing their original tracker/setup evidence remain ineligible; the
supplement producer cannot fabricate either proof.

### Captured parent inventory from actual verification

`check_admitted_node_operation` returns a parent tuple only after the exact
operation passes signature, parent-state and admitted node-policy verification.
The tuple takes its commitment from that authenticated intermediate state and
its public policy from the independent admitted inventory. Conflicting original
node policy, absent node material and unsupported later epochs fail closed.

`VerifiedEnrollmentCommittedTransition` retains baseline and captured suffix
parents. `VerifiedEnrollmentTreeExtension` carries the resulting verified parent
inventory and original manifest digest; it remains nonserializable crypto
evidence, not current activation/freshness authority. The archive collector
consumes this strongest reference, verifies original manifest/policy ownership,
and has no pending-epoch or ambient-package fallback. Original immutable v2
archive encoding remains unchanged: its full-history digest and admitted policy
are reverified before captured heads are reconstructed.

The actual committed-confirmation fixture checks the intermediate original
epoch created by AddLeaf, the captured RotateEpoch parent and rejection of that
old fence as a pending-epoch successor. Existing divergent-prefix and foreign
runtime negatives remain. This does not close genuine distributed quorum,
nonroot inventory production or independent postcommit freshness scope.

Registered enrollment window notice binding uses `OnceLock<Arc<...>>` for its
single assignment. Candidate construction performs no awaited work inside cell
initialization; all repeated bindings still require exact original
manifest/transcript/expiration equality. The observed Arc does not duplicate the
terminal or signing owner. Strict Clippy's disallowed blocking mutex rule and
actual public cancellation/retry coverage enforce this boundary.

### Current enrollment identity signing ownership

Required enrollment identity selection validates the actual active epoch, ordered physical-device signing policy, exact public package, and locally retained encrypted participant package. A valid threshold share does not produce a solo identity capability. It returns the retained `RequiredSigningParticipantError::QuorumOwnerRequired` source through the native Service category; malformed policy, missing material, codec failures, and private/public mismatch retain their own failure categories. The actual finalizer regression exercises genuine transition from single-device bootstrap to threshold policy before selecting the next signing owner. The connected second-issuance regression remains required until an owned multi-party manifest signature and corresponding confirmation owner are integrated.

The required reader separates a domain-valid threshold-one policy from FROST backend support. `BackendThresholdUnsupported` identifies the unsupported retained policy without inventing a provider error; deterministic dependency coverage requires the actual dealer call to return native `InvalidMinSigners`. Required material loss is independently tested through the actual selected-provider backing fault and remains Storage, rather than quorum-unavailable Service.

Registered execution-window admission accepts only the original
`RegisteredEnrollmentGenerationCapability`. A ceremony ID cannot reacquire an
execution window. Pre-live allocation clock tests exercise their held reservation;
registered execution tests first issue the real signed invitation and preserve its
tracker/runtime binding through window admission.

Unregistered original-generation continuation consumes the actual held first-
decision recovery producer. Its returned reservation retains the original
runtime generation and tree guards through exact selector validation and
registration completion. Duplicate reconstruction in the caller is removed;
public canonical invitation evidence remains required before the registered
capability can be published. Library strict Clippy rejects unused production
ownership seams; changed interrupted-resume integration requires real coverage.

Compile-fail test harnesses share aura-testkit's workspace process lock with app and signals suites. Descriptor custody releases on process exit, and bounded acquisition retains native IO or contention causes. Tests never remove/recreate the lock inode or create a parallel suite-specific lock namespace. See docs/804_testing_guide.md.

### Crypto RNG clone ownership

`CryptoRng::Deterministic` retains its original `Arc<Mutex<StdRng>>`; subsystem clones share custody rather than copy generator state. Synchronous draw methods release the mutex before any awaited work. Native ownership coverage requires interleaved clone draws and post-original-drop continuation to equal an independently seeded reference stream, plus independently seeded runtime reproducibility. Production thread-local entropy behavior is unchanged. No fallible provider operation is introduced by this ownership correction.

### Exact capability declaration evidence

A capability boundary declares its exact capability type in a parsed input or output. Semantic labels may specify `capability_type = Type`; labels and body text do not establish custody. Accessors return that exact type, authorizers retain that typed input or output, and proof issuers also declare their authoritative proof source. Runtime helpers with an actual held receiver may specify `receiver_type = OwnerType`; expansion checks the concrete receiver against that type. This receiver contract does not apply to free functions or replace authorization inputs in authorizers.

Constants, capability-like substrings, incidental body calls, phantom markers and associated projections do not satisfy the declaration. The declaration verifies API shape; private constructors and actual runtime ownership validation establish authority. Pure validators, pure execution-plan builders and observed projections are not capability issuers and carry no decorative capability-boundary declarations. Their domain tests and effect-placement rules remain required.

Enrollment-owned wrapping births retain the authenticated rotation plan and
original immutable generation scope. Private child capabilities bind to the
actual shared runtime allocation registry. Failed/orphan cleanup retains original
first-decision and generation custody through provider retirement ACK; activation
retains positive custody. Recovery reconciles unhanded original pre-live births,
never reconstructs once-live secrets or upgrades legacy permanent records.

### Custom provider custody and dispatch

The complete custom typestate builder transfers its configured crypto, ordinary
storage, random, console and bounded transport inventory before subsystem
assembly in both asynchronous and synchronous construction. Persistent tree,
sync, leakage, authorization and journal owners retain the same selected
handlers; ordinary storage remains beneath unified encrypted storage. Secure
allocation lifetimes retain the concrete selected profile provider and its
physical brand independently of ordinary storage customization. Runtime random
draws and receipt initialization use the configured random owner. Configured
crypto failures never select default primitives. Production rejects a simulated
crypto handler. The random trait is infallible and has no entropy-quality probe;
the supplied provider is responsible for its cryptographic contract.

Configured transport selection uses declaration order and chooses the first
provider reporting an established channel, otherwise the first configured
provider. A send failure never changes providers or enters native/shared
transport. Receives inspect the bounded configured inventory in declaration
order; only typed `NoMessage` permits the next provider. Provider receive calls
must obey the trait's no-message contract; blocking provider implementations
can delay subsequent providers. Native sources remain attached to crypto,
console and storage failures. Transport errors retain the existing typed domain
value; its diagnostic-only variants do not gain fabricated native causes.

`tests/custom_provider_fidelity.rs` exercises actual custom async/sync assembly,
encrypted selected storage, configured entropy/console, native outages and
stable transport failure selection. Required execution belongs to the ownership
aggregate; zero/ignored tests do not satisfy provider fidelity.

Configured receive consumers drain their matching retained inbox before physical
provider access. Device/content-specific receive pumps physical-only ingress
after retained absence; unmatched frames remain with their existing runtime or
choreography queue owner. Provider faults cannot hide an already-retained
matching frame, and retained unrelated frames cannot cycle and starve new
ingress. Receipt/source/context/physical-recipient checks precede delivery.
The interleaved custom-ingress regression verifies these real consumer paths.

### Physical integration profiles in Testing mode

Ordinary Testing assembly does not own a selected physical profile and cannot authorize allocation-lifetime operations. The native unit-test adapter accepts a move-only `TestingOwnedProfileCapability`, acquired from the actual profile handler before provider construction and signing bootstrap. The adapter preserves shared transport and configured custom providers, and the runtime retains that exact physical lease and selected lifetime registry. Production-lease ingress still rejects nonproduction modes. A foreign configuration fails before selecting its secure provider. Restart fixtures drain and drop the old runtime, acquire the original physical profile again, and recover original ledger evidence; they never fabricate a root or treat missing custody as success. Native keyring and browser lifetime support have their separate provider contracts.

### Canonical participant envelope reader

Threshold signing service retrieval delegates to the runtime crypto owner's
versioned participant envelope reader. Version 1 binds authority, epoch and
participant through metadata and authenticated encryption. Version 2 additionally
requires the original allocation owner and selected-provider lifetime custody.
A service cannot reinterpret an owned envelope as a legacy package or accept raw
package bytes as compatibility evidence. Real enrollment retention and restart
fixtures exercise the version 2 producer and this shared consumer.

The required identity-key reader retains its exact runtime, epoch and physical
participant witness through canonical envelope decryption. Every supported
envelope version admits at most 131072 encoded bytes and a nonempty ciphertext
of at most 65536 bytes before cryptographic work. Bound violations retain their
native structural cause and cannot select a companion or older epoch. The
required bootstrap corruption regression exercises actual codec, ciphertext
bounds and authentication failures without fallback.

Rendezvous descriptor and channel handlers select the active physical identity
once, retain its runtime-issued signing context, and require its exact package.
Contact response publication follows the same rule: missing identity material
is a required typed failure, not successful omission of a signed response.
Neither path chooses a historical epoch after active-policy or package failure.

Rendezvous manager cryptographic ingress requires the actual runtime effect
system, selects one active physical identity context, and carries it into the
required canonical package reader. Storage-only mock providers and historical
epoch scans cannot authorize descriptor or Noise preparation. Native identity
failures retain their source chain and prevent descriptor materialization.
The required manager corruption regression uses real bootstrap and a present
canonical companion to prove that failure does not select alternate material.

Nonproduction default-profile factories return only a newly and exclusively
created isolated namespace. Exhausted collisions and native creation faults
return retained errors; an unchecked fallback path is never selected. Explicit
persistent-profile configuration remains an owned caller decision.

### Original non-enrollment invitation signing ownership

Contact, guardian and channel invitation creation retains one original physical
signer identity from the actual fresh `ReservedInvitationIssuance` before local
publication. Protected immutable public metadata binds the exact original
invitation, recipient, context, creation time, device, epoch and verifier. Export
and response selection are load-only: missing original evidence cannot select
the active or another historical epoch or initialize replacement metadata.
`IssuedInvitationIdentityCapability` retains either the original required owned
sender record or a scoped borrow of the actual dispatch runtime and its required
canonical sender record. Plain deserialized records never become the capability.
Crypto reads use the existing exact physical identity context and canonical
versioned package owner; IO, codec and signing errors retain native causes.
Existing enrollment export continues to require its distinct pinned manifest
and original enrollment window. Legacy raw-package/history lookup helpers exist
only in test compilation.

Contact response signing and verification invoke the actual crypto provider on
the canonical typed transcript. Verifier/provider failures retain their native
source and cannot become a false signature result or an ignored response. Only
a successfully evaluated invalid signature is ordinary rejection evidence.

## Runtime shutdown completion owner

The activity handle exposes observation and admission closure, while stopped-state publication is private to RuntimeSystem. Already-closed admission is a native shutdown failure, not teardown evidence. The public agent facade retains native shutdown causes. Required native lifecycle execution includes the real agent already-closed shutdown regression; paired activity-handle doctests prove observation remains usable and stopped-state publication is unavailable through the public getter. These contracts do not replace complete operation/service drainage or original provider transfer.

### Required Contact confirmation custody

Contact acceptance derives its confirmation child from the original operation budget. Signed payload retries share that child, its physical observation owner and fixed deadline. Required imported metadata reads propagate storage, codec, identity and size failures; observed caches cannot substitute for those reads. A runtime-owned decision lease covers verified response selection through contact fact and imported status publication. The move-only verified response carries the retained complete import, so materialization does not re-resolve an invitation id. Native clock, transport and sleep faults retain their original standard error sources. Only an exact destination-unreachable fault permits acceptance retransmission.

Contact fact publication does not wait for an unrelated later view batch. Canonical commit and observed handler cache recording complete under the decision lease; app semantic readiness remains owned by its bounded authoritative refresh.

### Required threshold identity boundary regression

The real activated-enrollment identity test distinguishes a valid native threshold identity requiring the quorum service from malformed public-package bytes, unsupported native policy and actual local-share backing loss. Its required native lifecycle suite executes the exact named test; concrete native decoder, quorum-owner and missing-record causes survive the bridge. This classification boundary does not implement distributed signing.

### Guardian recovery pair custody

First-binding Guardian acceptance recording requires the retained
`IssuedInvitationIdentityCapability`. The boundary checks its actual runtime,
Guardian subject and original sender verifier before checking the response's
recovery-key possession proof. A raw invitation or response-carried issuer key
cannot replace that owner. Recording a recovery key does not confer device
membership. The actual regression creates the canonical invitation and rejects
foreign runtimes and substituted issuer or recovery keys before publication.

The runtime owns the private Guardian keypair lease; invitation code obtains a
move-only identity capability after required native reads, pair validation, or
both acknowledged fresh writes. Private bytes remain in `Zeroizing` storage.
Partial original key loss, mismatched retained keys and foreign runtime authority
fail with typed source chains; none authorizes a replacement pair. Required
Guardian response signing/verification uses canonical source-preserving
transcript encoding and actual selected cryptographic effects. The lifecycle
inventory executes actual owned-profile reopen/concurrent pair and native
failure regressions. Interrupted fresh publication and historical loss of both
halves still require independent durable lifetime evidence.

Regular imported invitation decision custody is shared by Contact and Guardian
through `ImportedInvitationDecisionLeaseCapability`, issued only by the original
runtime. Guardian required response reads bounded original metadata and preserves
native codec/read failure before key allocation. Its receiver-local materialized
context is distinct from the retained sender code context; payload binding must
retain the latter as original import evidence. Required VM/clock/codec/storage
failures keep native causes. Original public-operation windows and acknowledged
session teardown remain separate required boundaries.

Guardian protocol entry owns one physical operation window before required
preparation. The receiver's imported decision-lease wait, keypair preparation,
VM admission and VM loop are bounded by this original window; helpers borrow
its shared observation owner. Required held-import deadline and native timer
failure coverage runs in the VM lifecycle inventory. Timeout/drop is not
session-close acknowledgment or completed runtime drain.

Guardian terminal processing consumes `OwnedVmSession` and checks its native
close result. Failed primary+close retains both typed local causes, with primary
source traversal and timeout category preserved. The required regression uses
actual retired runtime owner and VM ingress/close, rather than a mocked close
result. Outer cancellation and full runtime drain remain separate boundaries.

### Retained public admission and original shutdown resource owner

The actual effects assembly and RuntimeSystem share one private atomic admission
owner. Retained public mutation leases span awaits in invitation,
authentication, chat, OTA, recovery and sessions; nested recovery continuations
borrow their original lease. Resource capacity is 256 concurrent operations.
The advanced admitted-effects facade retains its original lease and checks both
runtime gate and effects identity; it does not remove all legacy raw escapes.

The shutdown resource window is born once at shutdown entry (30 seconds).
Admission closes before drain; admitted operations settle before scheduler stop.
Scheduler disposal closes and drains accepted fact ingress before natural
completion; it does not cancel the callback before its final publication ACK.
TaskGroup and TaskSupervisor original-budget disposal reuse that same physical
window and acknowledge actual callback destruction. Clock/deadline/native
cleanup failures remain observable and prevent stopped success. A count-only
drain capability cannot authorize selected-provider or secret-registry transfer.
The required native lifecycle gate inventories and executes the finite capacity,
foreign-profile, stale service, cancellation, original-clock and real descendant
regressions; discovery alone does not satisfy enforcement.

## Original runtime service-stop continuation

Successful public admission closure creates a move-only shutdown window tied to
the actual gate, effect system and task root. Required service disposal and health
acknowledgments consume that same original resource window; the common service
helper cannot allocate another deadline. Authority termination is published only
after all required pipeline, task-tree, service-health and lifecycle acknowledgments,
within the original window.
Provider, deadline and authority-state failures retain their native causes and
withhold successful runtime termination.

This contract does not authorize profile transfer. Internal service cleanup
windows and whole provider/registry/RNG reassembly require their own completed
owner integration before a full handoff can be acknowledged.

### Exact reactive processing ownership

Reactive scheduler ingress issues ordered publication targets only after actual enqueue, retaining its original identity in both envelope and target. Required processing uses a retained watch highwater acknowledged after all actual registered view updates. Native scheduler failure is retained alongside completed highwater; completed targets stay completed while pending targets receive that native fault. Canonical required commits carry their actual stored facts and exact target. Chat operations retain one admitted runtime lease and one original local resource window through all mutation and processing awaits. Runtime startup carries one shared original resource window through service start, health and initial replay; partial startup owns reverse cleanup, retaining secondary faults. Optional initial LAN descriptor work remains in the same supervisor and original window after primary readiness.

Guardian recovery-key continuity uses a private typed transcript under
`aura.guardian.recovery-keypair`, schema 1, with the original authority/public-key
payload. Both signing and verification use the required native-source helpers.
This local proof checks retained pair integrity; it grants no acceptance,
replacement-key birth or independent historical-loss recovery authority.

Contact response verification borrows a private required-response capability
that retains the original imported record, awaited acceptance digest, actual
handler/effects identity and decision lease. Its key accessor checks the same
physical runtime owner before returning the original code key. Successful
verification transfers that same record and lease into the terminal response
owner; a raw key, copied record or foreign effects reference cannot replace it.
This continuity grants no verified device-membership authority.

Required local Guardian pair verification retains its actual keypair lease and
original required reads in a private move-owned capability until continuity
verification completes. The native primitive borrows that owner's verifier;
matching authority ids cannot substitute another runtime. The lease is released
after verification, while the validated signing identity retains its original
key material. It is not extended into the returned identity across unrelated
pair reads. The required regression uses two genuinely owned profiles under the
same authority to verify this runtime distinction.

Guardian imported-code confirmation verification retains the bounded original
import record and actual decision lease in a private required capability. After
binding the invocation to original imported fields, the native primitive borrows
only the original record's sender verifier through that capability's runtime
check. The acceptance payload may carry a copy for transcript binding; that copy
is not verification authority. This continuity role stays distinct from local
pair integrity and first-binding recovery-key possession.

### Registered Sync command ownership

`RuntimeSystem` issues `AdmittedSyncCommandCapability` after startup using its
actual public-operation lease, effects and task root. The registered command
survives foreground cancellation in `SyncCommandRegistryService`; the startup
lease is released after that handoff. Each foreground or periodic round obtains
one bounded operation from the same runtime. Requested peers are never omitted
by recent-attempt suppression. No tracked peers is an idle disposition, not
successful peer synchronization.

The registry stops the actual manager and waits for its exact issued task groups
under the original shutdown capability before reactive processing or root tasks
are stopped. A stopped flag or observed health is not a cleanup acknowledgment.
Command-local stop has its own bounded cleanup policy; it cannot mint runtime
shutdown authority. Required protocol, time, cleanup and supervision errors
retain original native causes. Exact local session removal is not remote
transport teardown evidence.

### Execution-mode entropy custody

The private `NonProductionEntropySeed` is minted only by the checked actual-mode factory. Both seeded crypto and the shared deterministic RNG stream borrow it; custom real crypto does not authorize seeded production randomness. Invalid production seed configuration retains `ProductionSeededEntropyError` before profile IO, instead of panicking after custody acquisition. Required tests exercise the complete constructor with a custom real crypto handler and verify no selected provider is called.

Sync command admission carries the configured journal/anti-entropy protocol
policy into the actual L5 service. Retry configuration changes attempt resource
use but cannot extend the admitted original operation window. Native-fault
fixtures select a single actual provider attempt, retaining the real protocol,
transport, task and session owners.

## Raw threshold signing boundary

The generic `SigningContext` service and enrollment setup export validate actual current native policy and the local physical participant package before reporting a missing quorum owner. These raw inputs cannot authorize reading other participants' private shares, allocating a distributed round, or aggregating a group signature. Valid threshold state reports the native quorum-owner requirement; missing or malformed required local state retains its original storage or crypto cause. A successful threshold operation requires the separate admitted distributed producer, with each runtime retaining only its own share and one-use nonce.

### Required projection failures

`ReactiveView` callbacks and `ViewAdapter` delta application return a typed
`Result<(), AuraError>`. The scheduler stops a failed batch before later views,
processing acknowledgment, or successful batch diagnostics. The original
failure is retained in the processing progress channel for issued target
observers. Signal snapshot and publication failures retain their concrete
`ReactiveError`; display-only error publication cannot establish completion.

Failed service startup can retain partial runtime resources. The lifecycle permits
`Failed -> Stopping -> Stopped` so their original owner can dispose them; failed
startup alone is not a disposal acknowledgment. The required startup replay
clock-failure regression exercises this cleanup path.

Original shutdown service progress acknowledgment uses the core bounded terminal
observation helper after the private service owner supplies actual stop/task
proof. The entire final observation is raced against the original fixed endpoint;
no unbounded clock read or fallible validation follows its synchronous callback.
AuraEffectSystem and EnhancedTimeHandler delegate absolute waits to the same
configured provider. This boundary alone does not bound initial shutdown-window
allocation or replace required service cleanup custody.
