# Choreography Development Guide

This guide covers how to build distributed protocols using Aura's choreographic programming system. Use it when you need to coordinate multiple parties with session types, CRDTs, and multi-phase workflows.

For theoretical foundations, see [MPST and Choreography](110_mpst_and_choreography.md). For operation categorization, see [Operation Categories](109_operation_categories.md).

## 1. When to Use Choreography

Use choreographic protocols when:
- Multiple parties must coordinate (threshold signing, consensus, sync)
- Session guarantees matter (no deadlock, no message mismatch)
- You need formal verification of protocol correctness

Do not use choreography for:
- Single-party operations (use effect handlers)
- Simple request-response (use direct transport)

## 2. Protocol Development Pipeline

This pipeline applies to all Layer 4/5 choreographies and all Category C ceremonies.

### Phase 1: Classification and Facts

Operation categories (A, B, C) are defined in [Operation Categories](109_operation_categories.md). The category determines coordination requirements and affects protocol design choices (local vs. CRDT vs. ceremony).

Define fact types with schema versioning:

```rust
use aura_macros::ceremony_facts;

#[ceremony_facts]
pub enum InvitationFact {
    CeremonyInitiated {
        ceremony_id: CeremonyId,
        agreement_mode: Option<AgreementMode>,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonyCommitted {
        ceremony_id: CeremonyId,
        relationship_id: String,
        agreement_mode: Option<AgreementMode>,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonyAborted {
        ceremony_id: CeremonyId,
        reason: String,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
}
```

The macro provides canonical `ceremony_id()` and `ceremony_timestamp_ms()` accessors.

### Phase 2: Choreography Specification

Write the choreography in a `.tell` file:

```rust
use aura_macros::choreography;

choreography! {
    #[namespace = "secure_request"]
    protocol SecureRequest {
        roles: Client, Server;

        Client[guard_capability = "chat:message:send", flow_cost = 50]
        -> Server: SendRequest(RequestData);

        Server[guard_capability = "chat:message:send", flow_cost = 30, journal_facts = "response_sent"]
        -> Client: SendResponse(ResponseData);
    }
}
```

Annotation syntax: `Role[guard_capability = "namespace:capability", flow_cost = N, journal_facts = "..."] -> Target: Message`

`guard_capability` is the one sanctioned raw-string boundary for first-party
capability names in choreography source. The macro parses these values into
validated `CapabilityName`s and fails closed on invalid or legacy names. In
Rust code outside `.tell` or `choreography!` inputs, prefer typed capability
families from the owning crate.

### Canonical Telltale 10 Shape

Aura choreographies should converge on one source shape before theorem-pack
rollout expands.

The canonical shape is:

- one `module ... exposing (...)` header
- one `protocol ... =` declaration
- one compact `roles ...` declaration using scalar roles or role families like
  `Devices[N]`
- message edges whose semantics live in the protocol structure itself:
  sequencing, fan-out, and `choice at ...`
- sanctioned Aura edge metadata only where it still represents real policy or
  accounting at the source boundary:
  `guard_capability`
  `flow_cost`
  `journal_facts`
  `leak`
  `parallel`
- no choreography-local ownership or migration scaffolding that should instead
  live in Telltale runtime admission, runtime transition artifacts, or Aura's
  Rust-side protocol/runtime glue

Use [crates/aura-sync/src/protocols/ota_activation.tell](/Users/hxrts/projects/aura/crates/aura-sync/src/protocols/ota_activation.tell)
and
[crates/aura-sync/src/protocols/device_epoch_rotation.tell](/Users/hxrts/projects/aura/crates/aura-sync/src/protocols/device_epoch_rotation.tell)
as the current style references.

#### Legacy-Only Source Surfaces To Remove

The following source patterns are migration debt and should disappear from
first-party `.tell` files:

- `link = "bundle=...|exports=...|imports=..."` when it is only preserving
  historical migration lore rather than declaring a live runtime fragment
  bundle consumed by reconfiguration or ownership code
- choreography comments whose only purpose is to preserve pre-Telltale runtime
  bundle/link ownership lore
- `journal_merge = true` as a choreography-local escape hatch
- `leakage_budget = "..."` tuple syntax
- repeated phase comments that restate obvious linear protocol structure rather
  than documenting a real protocol invariant

These semantics should not remain embedded in source once the runtime and
admission layers own them explicitly.

#### Aura Annotations That Remain Valid On The Clean Surface

These annotations remain valid on the canonical surface because they still map
to real Aura admission, accounting, or observation behavior:

- `guard_capability`
- `flow_cost`
- `journal_facts`
- `leak`
- `parallel`

Later theorem-pack phases may add Telltale-native theorem-pack declarations and
`requires` clauses, but those are additive and should not bring back legacy
bundle or migration hints.

#### Semantics That Must Move Out Of Source Over Time

The following concerns should move out of comments or source-local hints and
into proper Telltale 10 DSL/runtime constructs or Aura runtime glue:

- reconfiguration and runtime-upgrade bundle ownership
- fragment/link transfer semantics
- journal-merge behavior that is really runtime transition behavior
- recovery or membership handoff lore expressed only in comments
- admission/evidence requirements that should become theorem packs and runtime
  admission checks

## 2.1 Current Choreography Convergence Inventory

This inventory defines the target for each current `.tell` file while Aura
converges on the canonical Telltale 10 shape.

### First-Party Protocols

- [crates/aura-agent/src/handlers/sessions/coordination.tell](/Users/hxrts/projects/aura/crates/aura-agent/src/handlers/sessions/coordination.tell):
  already structurally close; remove phase-restating comments and the
  legacy `journal_merge = true` source hint.
- [crates/aura-amp/src/choreography.tell](/Users/hxrts/projects/aura/crates/aura-amp/src/choreography.tell):
  already structurally close; normalize onto the same compact source style and
  remove redundant “data transmission / acknowledgment” comments.
- [crates/aura-authentication/src/dkd.tell](/Users/hxrts/projects/aura/crates/aura-authentication/src/dkd.tell):
  structurally close; remove phase-only comments and keep only sanctioned edge
  metadata.
- [crates/aura-authentication/src/guardian_auth_relational.tell](/Users/hxrts/projects/aura/crates/aura-authentication/src/guardian_auth_relational.tell):
  structurally close; remove phase-only comments and keep the protocol free of
  source-local coordination lore.
- [crates/aura-consensus/src/protocol/choreography.tell](/Users/hxrts/projects/aura/crates/aura-consensus/src/protocol/choreography.tell):
  close to canonical shape; preserve `parallel` and `leak`, but remove
  phase-restating comments and keep it theorem-pack-free until a later explicit
  decision.
- [crates/aura-invitation/src/protocol.device_enrollment.tell](/Users/hxrts/projects/aura/crates/aura-invitation/src/protocol.device_enrollment.tell):
  keep the live `device_migration` bundle contract, but remove surrounding
  migration comments and any pseudo-annotations that no longer have a runtime
  consumer.
- [crates/aura-invitation/src/protocol.guardian_invitation.tell](/Users/hxrts/projects/aura/crates/aura-invitation/src/protocol.guardian_invitation.tell):
  structurally close; remove phase-only comments and keep only sanctioned edge
  metadata.
- [crates/aura-invitation/src/protocol.invitation_exchange.tell](/Users/hxrts/projects/aura/crates/aura-invitation/src/protocol.invitation_exchange.tell):
  structurally close; remove phase-only comments and keep the source minimal.
- [crates/aura-recovery/src/guardian_ceremony.tell](/Users/hxrts/projects/aura/crates/aura-recovery/src/guardian_ceremony.tell):
  add sanctioned Aura metadata where this protocol still requires it, normalize
  branch names and comments, and remove the bare legacy look.
- [crates/aura-recovery/src/guardian_membership.tell](/Users/hxrts/projects/aura/crates/aura-recovery/src/guardian_membership.tell):
  keep the live `guardian_handoff` bundle contract, but remove
  `leakage_budget = ...`, `journal_merge = true`, and the surrounding
  migration comments.
- [crates/aura-recovery/src/guardian_setup.tell](/Users/hxrts/projects/aura/crates/aura-recovery/src/guardian_setup.tell):
  remove `leakage_budget = ...` and `journal_merge = true`, and strip
  phase-restating comments.
- [crates/aura-recovery/src/recovery_protocol.tell](/Users/hxrts/projects/aura/crates/aura-recovery/src/recovery_protocol.tell):
  structurally close; remove phase-only comments and keep the source ready for
  later theorem-pack adoption only if the recovery semantics justify it.
- [crates/aura-rendezvous/src/protocol.relayed_rendezvous.tell](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/protocol.relayed_rendezvous.tell):
  keep theorem-pack-free; normalize comments and preserve only real route-flow
  metadata like `guard_capability`, `flow_cost`, and `leak`.
- [crates/aura-rendezvous/src/protocol.rendezvous_exchange.tell](/Users/hxrts/projects/aura/crates/aura-rendezvous/src/protocol.rendezvous_exchange.tell):
  keep theorem-pack-free; normalize comments and preserve only real publication
  and connect metadata.
- [crates/aura-sync/src/protocols/epochs.tell](/Users/hxrts/projects/aura/crates/aura-sync/src/protocols/epochs.tell):
  keep the live `epoch_rotation_transfer` bundle contract, but remove the
  surrounding migration comments and keep the rest of the protocol in the same
  compact shape as the OTA and device-epoch references.

### Protocol-Compat Fixtures

- [crates/aura-testkit/fixtures/protocol_compat/breaking_baseline.tell](/Users/hxrts/projects/aura/crates/aura-testkit/fixtures/protocol_compat/breaking_baseline.tell):
  already canonical; no migration needed beyond keeping fixture style minimal.
- [crates/aura-testkit/fixtures/protocol_compat/breaking_current.tell](/Users/hxrts/projects/aura/crates/aura-testkit/fixtures/protocol_compat/breaking_current.tell):
  remove the explanatory inline comment so the compatibility edge is expressed
  only by the message-shape difference.
- [crates/aura-testkit/fixtures/protocol_compat/compatible_baseline.tell](/Users/hxrts/projects/aura/crates/aura-testkit/fixtures/protocol_compat/compatible_baseline.tell):
  already canonical; keep unchanged except for style parity if needed.
- [crates/aura-testkit/fixtures/protocol_compat/compatible_current.tell](/Users/hxrts/projects/aura/crates/aura-testkit/fixtures/protocol_compat/compatible_current.tell):
  already canonical; keep unchanged except for style parity if needed.

## 2.2 Theorem-Pack Taxonomy and Admission Policy

Aura should add theorem packs only where they drive a real runtime admission
decision. They are not importance labels.

The first production protocols now using this boundary are
`aura.sync.ota_activation` and `aura.sync.device_epoch_rotation`. Both declare
`AuraTransitionSafety`, and Aura rejects launch before protocol start when the
runtime lacks the theorem-pack capability surface needed for transition
receipts, bridge admission, and reconfiguration safety.

### Current Protocol Split

First-wave theorem-pack candidates:

- `aura.sync.ota_activation`
- `aura.sync.device_epoch_rotation`

Second-wave theorem-pack candidates:

- `aura.recovery.grant`
- `aura.recovery.guardian_setup`
- `aura.recovery.guardian_ceremony`
- `aura.recovery.guardian_membership_change`
- `aura.invitation.device_enrollment` once the recovery/finalization path is
  explicitly theorem-pack-gated end to end

Later explicit evaluation target:

- `aura.consensus`

Current theorem-pack-free protocols:

- `aura.session.coordination`
- `aura.amp.transport`
- `aura.dkg.ceremony`
- `aura.authentication.guardian_auth_relational`
- `aura.invitation.exchange`
- `aura.invitation.guardian`
- `aura.rendezvous.exchange`
- `aura.rendezvous.relay`
- `aura.sync.epoch_rotation`

### Current Aura Runtime Admission Surfaces

Aura already has a small runtime capability bridge and several concrete runtime
consumers:

- `aura-core::effects::RuntimeCapabilityEffects` defines the stable admission
  interface.
- `aura-effects::RuntimeCapabilityHandler` maps startup runtime contracts into a
  capability inventory and public protocol-critical surfaces.
- `aura-protocol::admission` maps protocol ids to current required runtime
  capability keys.
- `aura-agent::runtime::choreo_engine` and
  `aura-agent::runtime::choreography_adapter` enforce required runtime
  capability admission before protocol launch.
- `aura-agent::runtime::contracts` and
  `aura-agent::runtime::vm_hardening` already validate runtime capability
  requirements for determinism and profile policy.
- `aura-agent::runtime::services::threshold_signing` and
  `aura-agent::runtime::services::reconfiguration_manager` already consume
  protocol-critical runtime capability state on concrete execution paths.

The currently published public protocol-critical surfaces are:

- `runtime_admission`
- `theorem_pack_capabilities`
- `ownership_capability`
- `readiness_witness`
- `authoritative_read`
- `materialization_proof`
- `canonical_handle`
- `ownership_receipt`
- `semantic_handoff`
- `reconfiguration_transition`

These are the only current Aura-owned runtime consumers theorem-pack mapping is
allowed to target.

### Initial Aura Theorem-Pack Taxonomy

Aura should keep the first taxonomy small:

- `AuraTransitionSafety`
  used for OTA/runtime-upgrade/device-epoch flows that depend on transition
  safety, reconfiguration continuity, and bridge/admission correctness
- `AuraAuthorityEvidence`
  used for recovery/finalization flows that depend on authoritative read,
  canonical materialization, and receipt/evidence-backed transitions
- `AuraConsensusDeployment`
  reserved for a later explicit consensus decision; do not use until Aura has a
  concrete runtime consumer beyond the existing envelope capability checks

### Pack-to-Capability Mapping

`AuraTransitionSafety`:

- reuse Telltale inventory keys:
  `protocol_machine_envelope_adherence`,
  `protocol_machine_envelope_admission`,
  `protocol_envelope_bridge`,
  `reconfiguration_safety`
- current Aura runtime consumers:
  `theorem_pack_capabilities`,
  `ownership_receipt`,
  `semantic_handoff`,
  `reconfiguration_transition`
- concrete admission checks:
  `aura-agent::runtime::choreo_engine`,
  `aura-agent::runtime::choreography_adapter`,
  `aura-agent::runtime::services::reconfiguration_manager`

`AuraAuthorityEvidence`:

- reuse Telltale inventory keys:
  `protocol_machine_envelope_adherence`,
  `protocol_machine_envelope_admission`,
  `protocol_envelope_bridge`
- Aura-owned runtime capability names already backed by real consumers:
  `authoritative_read`,
  `materialization_proof`,
  `canonical_handle`
- concrete admission checks:
  `aura-agent::runtime::choreo_engine`,
  `aura-agent::runtime::contracts`,
  recovery/guardian launch paths in `aura-agent`
- current adopted production protocols:
  `aura.recovery.grant`,
  `aura.recovery.guardian_setup`,
  `aura.recovery.guardian_ceremony`
- current explicit non-goals in the recovery/invitation area:
  `aura.recovery.guardian_membership_change`,
  `aura.invitation.guardian`,
  `aura.invitation.device_enrollment`

`AuraConsensusDeployment`:

- candidate Telltale inventory keys:
  `consensus_envelope`,
  `atomic_broadcast_ordering`,
  `partial_synchrony_liveness`
- current status:
  deferred until Aura adds a runtime admission consumer beyond
  `byzantine_envelope` / determinism profile checks

Aura-specific capability names are only allowed when they already have a real
runtime consumer. Today that means `authoritative_read`,
`materialization_proof`, and `canonical_handle` are allowed. New Aura-specific
theorem-pack keys should not be added until the corresponding runtime admission
check exists.

### Admission Boundary

Aura consumes theorem-pack metadata only through the generated
`CompositionManifest` boundary:

- generated manifests carry theorem-pack declarations
- generated manifests carry required theorem-pack names
- generated manifests carry the flattened required theorem-pack capability set

`aura-protocol::admission` is the single Aura-owned translation layer that maps
required theorem packs onto concrete runtime-admission requirements. That layer
must stay small and fail closed:

- a required theorem pack with no matching generated declaration is rejected
- a required theorem pack with no Aura admission policy is rejected
- a theorem-pack declaration whose declared capability set drifts from Aura's
  supported taxonomy is rejected
- runtime launch rejects missing required theorem-pack capability coverage
  before protocol execution begins

`aura-agent::runtime::vm_host_bridge`, `aura-agent::runtime::choreo_engine`,
and `aura-agent::runtime::choreography_adapter` consume that resolved boundary;
they do not invent parallel theorem-pack policy tables.

### Acceptance Rule

A choreography may add theorem-pack requirements only when all of the following
are true:

- the protocol depends on protocol-critical authority, evidence, or transition
  semantics
- the required pack maps to existing Telltale inventory keys or to an
  Aura-specific runtime capability with a real admission consumer
- Aura runtime admission fails closed before launch when that support is absent
- a negative test proves the missing-pack admission failure
- a positive test proves the protocol still runs when the support is present

If any of those conditions is missing, keep the choreography theorem-pack-free.

### Migration Rule

When converting a choreography, prefer:

- deleting legacy source hints rather than renaming them
- moving ownership or transition semantics into Rust/runtime admission when they
  are not true choreography structure
- keeping theorem packs out until the protocol has a real runtime admission
  consumer
- matching the source compactness of the OTA and device-epoch references

Select the narrowest `TimeStamp` domain for each time field. See [Effect System](103_effect_system.md) for time domains.

### Phase 3: Runtime Wiring

Create the protocol implementation:

```rust
use aura_agent::runtime::open_manifest_vm_session_admitted;

let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
    &my_protocol::COMPOSITION_MANIFEST,
    "Initiator",
    &my_protocol::global_type(),
    &my_protocol::local_types(),
    scheduler_signals,
).await?;

let status = engine.run_to_completion(vm_sid)?;
```

This wiring opens an admitted VM session from generated choreography metadata. The runtime source of truth is the composition manifest, not an ad hoc adapter. Register the service with the runtime and integrate it with the guard chain. Category C operations must follow the ceremony contract.

Production services should treat the admitted unit as a protocol fragment. If the manifest declares link bundles, each linked bundle becomes its own ownership unit. Runtime transfer must use `ReconfigurationManager`. Do not bypass fragment ownership through service-local state.

The runtime also derives execution mode from admitted policy. Cooperative protocols stay on the canonical VM path. Replay-deterministic and envelope-bounded protocols select the threaded path only through the admission and hardening surface. Service code should not construct ad hoc threaded runtimes.

### Phase 4: Status and Testing

Implement `CeremonyStatus` for Category C or protocol-specific status views:

```rust
pub fn ceremony_status(facts: &[InvitationFact]) -> CeremonyStatus {
    // Reduce facts to current status
}
```

Definition of Done:
- [ ] Operation category declared (A/B/C)
- [ ] Facts defined with reducer and schema version
- [ ] Choreography specified with roles/messages documented
- [ ] Runtime wiring added (role runners + registration)
- [ ] Fragment ownership uses manifest admission and runtime ownership APIs
- [ ] `delegate` and `link` flows use `ReconfigurationManager`
- [ ] Threaded or envelope-bounded execution uses admitted policy only
- [ ] Category C uses ceremony runner and emits standard facts
- [ ] Status output implemented
- [ ] Shared-bus integration test added
- [ ] Simulation test added
- [ ] Choreography parity/replay tests added (Category C)

See `crates/aura-consensus/src/protocol/` for canonical examples.

## 3. CRDT Integration

CRDTs handle state consistency in choreographic protocols. See [Journal](105_journal.md) for CRDT theory.

### CRDT Coordinator

Use `CrdtCoordinator` to manage CRDT state in protocols:

```rust
use aura_protocol::effects::crdt::CrdtCoordinator;

// State-based CRDT
let coordinator = CrdtCoordinator::with_cv_state(authority_id, initial_journal);

// Delta CRDT with compaction threshold
let coordinator = CrdtCoordinator::with_delta_threshold(authority_id, 100);

// Meet-semilattice for constraints
let coordinator = CrdtCoordinator::with_mv_state(authority_id, capability_set);
```

### Protocol Integration

Protocols consume and return coordinators with updated state:

```rust
use aura_sync::choreography::anti_entropy::execute_as_requester;

let (result, updated_coordinator) = execute_anti_entropy(
    authority_id,
    config,
    is_requester,
    &effect_system,
    coordinator,
).await?;

let synchronized_state = updated_coordinator.cv_handler().get_state();
```

## 4. Protocol Composition

Complex applications require composing multiple protocols.

### Sequential Composition

Chain protocols for multi-phase workflows:

```rust
pub async fn execute_authentication_flow(
    &self,
    target_device: aura_core::DeviceId,
) -> Result<AuthenticationResult, ProtocolError> {
    // Phase 1: Identity exchange
    let identity_result = self.execute_identity_exchange(target_device).await?;

    // Phase 2: Capability negotiation
    let capability_result = self.execute_capability_negotiation(
        target_device,
        &identity_result
    ).await?;

    // Phase 3: Session establishment
    let session_result = self.execute_session_establishment(
        target_device,
        &capability_result
    ).await?;

    Ok(AuthenticationResult {
        identity: identity_result,
        capabilities: capability_result,
        session: session_result,
    })
}
```

Each phase uses results from previous phases. Failed phases abort the entire workflow.

### Parallel Composition

Independent protocols can execute concurrently using `try_join_all`.

```rust
pub async fn execute_distributed_computation(
    &self,
    worker_devices: Vec<aura_core::DeviceId>,
) -> Result<ComputationResult, ProtocolError> {
    // Launch parallel worker protocols
    let worker_futures = worker_devices.iter().map(|device| {
        self.execute_worker_protocol(*device)
    });

    // Wait for all workers with timeout
    let worker_results = tokio::time::timeout(
        self.config.worker_timeout,
        futures::future::try_join_all(worker_futures)
    ).await??;

    // Aggregate results
    self.aggregate_worker_results(worker_results).await
}
```

Worker futures launch in parallel and are joined with a timeout. Results are then aggregated into a single computation result.

### Effect Program Composition

Protocols can also be composed through effect programs using a builder pattern.

```rust
let composed_protocol = Program::new()
    .ext(ValidateCapability {
        capability: "coordinate".into(),
        role: Coordinator
    })
    .then(anti_entropy_program)
    .then(threshold_ceremony_program)
    .ext(LogEvent {
        event: "protocols_complete".into()
    })
    .end();
```

The builder chains validation, protocol execution, and logging into a single composed program.

## 5. Error Handling and Resilience

### Timeout and Retry

Implement timeout handling with exponential backoff.

```rust
pub async fn execute_with_resilience<T>(
    &self,
    protocol_fn: impl Fn() -> BoxFuture<'_, Result<T, ProtocolError>>,
    operation_name: &str,
) -> Result<T, ProtocolError> {
    let mut attempt = 0;

    while attempt < self.config.max_attempts {
        match tokio::time::timeout(
            self.config.operation_timeout,
            protocol_fn()
        ).await {
            Ok(Ok(result)) => return Ok(result),
            Ok(Err(e)) if !e.is_retryable() => return Err(e),
            _ => {
                // Exponential backoff with jitter
                let delay = self.config.base_delay * 2_u32.pow(attempt);
                tokio::time::sleep(self.add_jitter(delay)).await;
                attempt += 1;
            }
        }
    }

    Err(ProtocolError::MaxRetriesExceeded)
}
```

The function retries on transient errors with exponential backoff and jitter. Non-retryable errors fail immediately.

### Compensation and Rollback

For multi-phase protocols, implement compensation for partial failures.

```rust
pub async fn execute_compensating_transaction(
    &self,
    operations: Vec<Operation>,
) -> Result<TransactionResult, TransactionError> {
    let mut completed = Vec::new();

    for operation in &operations {
        match self.execute_operation(operation).await {
            Ok(result) => {
                completed.push((operation.clone(), result));
            }
            Err(e) => {
                // Compensate in reverse order
                self.execute_compensation(&completed).await?;
                return Err(TransactionError::OperationFailed {
                    operation: operation.clone(),
                    cause: e,
                });
            }
        }
    }

    Ok(TransactionResult { completed })
}
```

On failure, completed operations are compensated in reverse order. This ensures partial state is cleaned up before the error is returned.

### Circuit Breakers

Circuit breakers prevent cascading failures by tracking error rates.

```rust
pub enum CircuitState {
    Closed { failure_count: usize },
    Open { opened_at: Instant },
    HalfOpen { test_requests: usize },
}

pub async fn execute_with_circuit_breaker<T>(
    &self,
    protocol_fn: impl Fn() -> BoxFuture<'_, Result<T, ProtocolError>>,
) -> Result<T, ProtocolError> {
    let should_execute = match &*self.circuit_state.lock() {
        CircuitState::Closed { failure_count } =>
            *failure_count < self.config.failure_threshold,
        CircuitState::Open { opened_at } =>
            opened_at.elapsed() >= self.config.recovery_timeout,
        CircuitState::HalfOpen { test_requests } =>
            *test_requests < self.config.test_threshold,
    };

    if !should_execute {
        return Err(ProtocolError::CircuitBreakerOpen);
    }

    match protocol_fn().await {
        Ok(result) => {
            self.record_success();
            Ok(result)
        }
        Err(e) => {
            self.record_failure();
            Err(e)
        }
    }
}
```

The breaker transitions through closed, open, and half-open states based on failure thresholds and recovery timeouts.

## 6. Guard Chain Integration

The guard chain specification is defined in [Authorization](106_authorization.md). See [System Internals Guide](807_system_internals_guide.md) for the three-phase implementation pattern.

When integrating guards into choreographies, use the annotation syntax on choreography messages. The annotations compile to guard chain commands that execute before transport sends:

- `guard_capability`: Creates capability check before send
- `flow_cost`: Charges flow budget
- `journal_facts`: Records facts after successful send
- `leak`: Records leakage budget charge

Snapshot builders must not treat declared choreography guard names as already
granted. They evaluate typed candidate sets against the current Biscuit/policy
frontier and publish only the admitted frontier into the `GuardSnapshot`.

## 7. Domain Service Pattern

Domain crates define stateless handlers. The agent layer wraps them with services.

### Domain Handler

```rust
// In domain crate (e.g., aura-chat/src/service.rs)
pub struct ChatHandler;

impl ChatHandler {
    pub async fn send_message<E>(
        &self,
        effects: &E,
        channel_id: ChannelId,
        content: String,
    ) -> Result<MessageId>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects
    {
        let message_id = effects.random_uuid().await;
        // ... domain logic
        Ok(message_id)
    }
}
```

### Agent Service Wrapper

```rust
// In aura-agent/src/handlers/chat_service.rs
pub struct ChatService {
    handler: ChatHandler,
    effects: Arc<RwLock<AuraEffectSystem>>,
}

impl ChatService {
    pub async fn send_message(
        &self,
        channel_id: ChannelId,
        content: String,
    ) -> AgentResult<MessageId> {
        let effects = self.effects.read().await;
        self.handler.send_message(&*effects, channel_id, content)
            .await
            .map_err(Into::into)
    }
}
```

This keeps the domain crate pure without Tokio-specific locking or runtime coupling. It is testable with mock effects and consistent across crates.

## 8. Testing Choreographies

### Unit Testing Guard Logic

```rust
#[test]
fn test_cap_guard_denies_unauthorized() {
    let snapshot = GuardSnapshot {
        capabilities: vec![],
        flow_budget: FlowBudget { limit: 100, spent: 0, epoch: 0 },
        ..Default::default()
    };
    let result = CapGuard::evaluate(&snapshot, &SendRequest::default());
    assert!(result.is_err());
}
```

### Integration Testing Protocols

```rust
#[aura_test]
async fn test_sync_protocol() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;

    let coordinator = CrdtCoordinator::with_cv_state(
        fixture.authority_id(),
        fixture.initial_journal(),
    );

    let (result, _) = execute_anti_entropy(
        fixture.authority_id(),
        SyncConfig::default(),
        true, // is_requester
        &fixture.effects(),
        coordinator,
    ).await?;

    assert!(result.is_success());
    Ok(())
}
```

### Simulation Testing

See [Simulation Guide](805_simulation_guide.md) for fault injection and adversarial testing.

## Related Documentation

- [MPST and Choreography](110_mpst_and_choreography.md) - Session type theory
- [Operation Categories](109_operation_categories.md) - Category A/B/C classification
- [Authorization](106_authorization.md) - Guard chain specification
- [Journal](105_journal.md) - CRDT and fact semantics
- [Testing Guide](804_testing_guide.md) - Test patterns
- [Simulation Guide](805_simulation_guide.md) - Fault injection testing
