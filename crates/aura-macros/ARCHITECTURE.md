# Aura Macros (Layer 2)

## Purpose

Compile-time DSL parser for choreographies with Aura-specific annotations. Generates type-safe Rust code for distributed protocols. Also hosts Rust-native syntax lints via `src/bin/arch_lints.rs` and ownership/runtime boundary lints via `src/bin/ownership_lints.rs`.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| `tell!` macro: Full Telltale feature inheritance with Aura extensions | Runtime code or effect implementations |
| `DomainFact` derive macro: Canonical encoding with schema versioning | Multi-party coordination (only generates code) |
| `aura_effect_handlers` macro: Mock/real handler variant boilerplate | |
| `aura_handler_adapters` macro: AuraHandler trait adapters | |
| `aura_test` attribute macro: Async test setup with tracing | |
| `src/bin/arch_lints.rs`: Rust-native syntax lints for `just lint-arch-syntax`, including shared frontend portability and semantic-bridge contract checks | |
| `src/bin/ownership_lints.rs`: Ownership/runtime boundary enforcement lints for `just ci-ownership-policy`, including frontend handoff, best-effort side-effect, and proof-bearing success boundaries | |
| Validated ownership marker attrs: `authoritative_source`, `strong_reference`, `weak_identifier`, `actor_root` | Unchecked ownership marker comments or ad hoc tags |

### Consolidation Rationale

`aura-macros` intentionally keeps the fact, choreography, effect, and
ownership macros in one proc-macro crate. A cold-ish local
`cargo check -p hxrts-aura-macros` measurement on 2026-04-19 completed in
about 31.5s real time, and the dominant cost was the shared proc-macro /
Telltale dependency stack rather than crate-local module boundaries. Splitting
the crate would duplicate version coordination and shared helper maintenance
without clearly removing that first-build cost, while the current consolidated
crate is compiled once and then cached across dependents.

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Inbound | `aura-core` | Domain types (compile-time only) |
| Inbound | Choreography protocol specifications | Token streams |
| Inbound | Domain fact enum definitions | Derive macro input |
| Outbound | Generated choreography/fact/handler surfaces | Consumed by downstream crates |

## Invariants

- Depends only on aura-core (pure compile-time code generation).
- Is a proc-macro crate (no runtime code).
- All work happens at compile time.
- Uses the shared Telltale frontend, then lowers compiled annotation metadata into Aura-owned semantics.

### InvariantChoreographyAnnotationProjection

Choreography annotations must project deterministically into runtime metadata.

Enforcement locus:
- src proc-macro parsing captures guard, flow, and leakage annotations.
- ownership marker attrs validate required metadata and target item shape.
- Expansion outputs remain compile-time only and avoid runtime side effects.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-macros
- just lint-arch-syntax
- just ci-ownership-policy

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines annotation semantics for guards and leakage.
- [MPST and Choreography](../../docs/110_mpst_and_choreography.md) defines projection expectations.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-macros` is primarily `Pure`. It owns compile-time translation, not `ActorOwned` runtime lifecycle. Macro output may expose `MoveOwned` or capability-gated contracts, but the macro crate does not own those lifecycles at runtime. `Observed` tooling may inspect expansions, not mutate semantic truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| proc-macro parsers and expanders in `src/` | `Pure` | Compile-time parsing and code generation only. |
| generated choreography/fact/handler surfaces | `Pure` producer | Macro output may encode `MoveOwned` and capability-gated contracts, but the macro crate does not own them at runtime. |
| Actor-owned runtime state | none | Proc-macro crates must not own runtime lifecycle or background tasks. |
| Observed-only surfaces | none | Macro inspection tooling lives outside the crate. |

### Capability-Gated Points

- Generated typed capability surfaces and ownership contracts consumed by downstream crates
- Compile-time validation for canonical capability-family declarations and choreography capability parsing boundaries

## Testing

### Strategy

aura-macros is a proc-macro crate — all work happens at compile time. The critical concern is that valid inputs compile and invalid inputs produce clear errors. If a valid choreography is rejected or an invalid one is silently accepted, the DSL contract is broken.

### Commands

```
cargo test -p aura-macros --test compile_fail  # boundary tests
cargo test -p aura-macros --lib                # inline unit tests
```

To regenerate `.stderr` files after intentional changes:
```
TRYBUILD=overwrite cargo test -p aura-macros --test compile_fail
```

### Coverage matrix

| What breaks if wrong | Test file | Status |
|---------------------|----------|--------|
| Valid choreography annotations rejected | `boundaries/valid_annotations.rs` | covered (pass) |
| Valid ceremony facts rejected | `boundaries/ceremony_facts_valid.rs` | covered (pass) |
| Valid semantic_owner rejected | `boundaries/semantic_owner_valid.rs` | covered (pass) |
| Valid actor_owned / actor_root rejected | `boundaries/actor_owned_valid.rs`, `boundaries/actor_root_valid.rs` | covered (pass) |
| Valid capability_boundary rejected | `boundaries/capability_boundary_valid.rs` | covered (pass) |
| Valid ownership_lifecycle rejected | `boundaries/ownership_lifecycle_valid.rs` | covered (pass) |
| Valid authoritative_source / strong_reference / weak_identifier rejected | `boundaries/authoritative_source_valid.rs`, `boundaries/strong_reference_valid.rs`, `boundaries/weak_identifier_valid.rs` | covered (pass) |
| Invalid flow_cost silently accepted | `boundaries/invalid_flow_cost.rs` | covered (compile_fail) |
| Invalid guard_capability accepted | `boundaries/invalid_guard_capability.rs` | covered (compile_fail) |
| Invalid generated canonical capability accepted | `boundaries/capability_family_invalid_generated_name.rs` | covered (compile_fail) |
| Macro/module namespace mismatch accepted | `boundaries/choreography_namespace_mismatch.rs` | covered (compile_fail) |
| Self-send accepted | `boundaries/incoherent_self_send.rs` | covered (compile_fail) |
| Missing namespace accepted | `boundaries/missing_namespace.rs` | covered (compile_fail) |
| semantic_owner missing context | `boundaries/semantic_owner_missing_context.rs` | covered (compile_fail) |
| semantic_owner missing owner | `boundaries/semantic_owner_missing_owner.rs` | covered (compile_fail) |
| semantic_owner missing category | `boundaries/semantic_owner_missing_category.rs` | covered (compile_fail) |
| semantic_owner missing terminal | `boundaries/semantic_owner_missing_terminal_path.rs` | covered (compile_fail) |
| actor_owned missing capacity | `boundaries/actor_owned_missing_capacity.rs` | covered (compile_fail) |
| actor_root missing supervision, invalid root name, or non-struct target | `boundaries/actor_root_missing_supervision.rs`, `boundaries/actor_root_invalid_name.rs`, `boundaries/actor_root_on_function.rs` | covered (compile_fail) |
| actor_owned missing gate | `boundaries/actor_owned_missing_gate.rs` | covered (compile_fail) |
| actor_owned bypass without macro | `boundaries/actor_owned_bypass_without_macro.rs` | covered (compile_fail) |
| actor_owned embeds move-owned or terminal publication field | `boundaries/actor_owned_forbidden_field.rs` | covered (compile_fail) |
| capability_boundary missing category or non-capability-bearing helper body | `boundaries/capability_boundary_missing_category.rs`, `boundaries/capability_boundary_non_capability_helper.rs` | covered (compile_fail) |
| ownership_lifecycle invalid variant | `boundaries/ownership_lifecycle_invalid_variant.rs` | covered (compile_fail) |
| authoritative_source metadata or target invalid | `boundaries/authoritative_source_missing_kind.rs`, `boundaries/authoritative_source_invalid_kind.rs`, `boundaries/authoritative_source_on_struct.rs` | covered (compile_fail) |
| strong_reference metadata or target invalid | `boundaries/strong_reference_missing_domain.rs`, `boundaries/strong_reference_invalid_domain.rs`, `boundaries/strong_reference_on_function.rs` | covered (compile_fail) |
| weak_identifier metadata or target invalid | `boundaries/weak_identifier_missing_domain.rs`, `boundaries/weak_identifier_invalid_domain.rs`, `boundaries/weak_identifier_on_function.rs` | covered (compile_fail) |

## References

- [MPST and Choreography](../../docs/110_mpst_and_choreography.md)
- [Theoretical Model](../../docs/002_theoretical_model.md)
- [Ownership Model](../../docs/122_ownership_model.md)

### Durable enrollment execution boundary

The `async-session-ownership` lane also enforces sealed durable enrollment windows. AST checks reject raw timeout executor calls or aliases, fresh timeout reconstruction, weaker attempt parameters, and raw budget methods in production device enrollment. Attempt functions and methods require a sealed window input regardless of parameter name. Test-only exclusions require a positive test predicate; `cfg(not(test))` remains checked. Adversarial fixtures cover each bypass, renamed inputs, missing inputs, and the sanctioned window path.

## Declared capability type recognition

Capability boundary declaration validation recognizes the exact configured capability type in the parsed signature, including references and nested result types. Parameter names, string literals and type-name substrings do not establish this exact signature match. Historical body-string, `_CAPABILITY` and substring shortcuts remain separate acceptance paths in the full validator; removing those unchecked paths is outstanding under Task 13. This syntax check does not establish construction provenance: opaque fields, actual provider and tracker identity checks, move ownership and internal negative trait assertions remain required. Original cleanup custody can own a real generation guard without granting enrollment publication or activation.

Production task-spawn enforcement excludes declarations only when parsed `cfg`
requires `test`. Conjunctions such as `all(test, unix)` qualify; `not(test)` and
`any(test, unix)` remain checked. Ownership CI requires exact source, discovery
and execution evidence for the positive and adversarial configuration regression.

All compile-fail suites share the host-only process lock implementation in
`toolkit/test-support/process_lock.rs`, including service-surface and marker
validation. Different suite names do not create different lock namespaces;
Cargo absence and bounded acquisition failure fail required coverage.

### Exact capability declaration evidence

A capability boundary declares its exact capability type in a parsed input or output. Semantic labels may specify `capability_type = Type`; labels and body text do not establish custody. Accessors return that exact type, authorizers retain that typed input or output, and proof issuers also declare their authoritative proof source. Runtime helpers with an actual held receiver may specify `receiver_type = OwnerType`; expansion checks the concrete receiver against that type. This receiver contract does not apply to free functions or replace authorization inputs in authorizers.

Constants, capability-like substrings, incidental body calls, phantom markers and associated projections do not satisfy the declaration. The declaration verifies API shape; private constructors and actual runtime ownership validation establish authority. Pure validators, pure execution-plan builders and observed projections are not capability issuers and carry no decorative capability-boundary declarations. Their domain tests and effect-placement rules remain required.

All ownership lint visitors use the common parsed test-only cfg predicate.
Attribute text containing `test` is insufficient: production-capable negations,
disjunctions and feature names continue to be enforced across policy domains.

Capability declaration matching retains full configured generic arguments:
qualified paths and their type parameters must match the actual signature.
The required full-validator regression rejects omitted/substituted parameters
and containers containing unrelated values.

The canonical agent result alias preserves success capability evidence. The
validator follows only its first success type, never diagnostic/error arms;
required positive and adversarial tests cover nested successful values.

Capability declaration matching rejects foreign qualified container lookalikes.
Preserve canonical wrapper paths, exact capability generic arguments, and the
required full-validator foreign Result/Arc/AgentResult regressions. Unqualified
names remain declaration syntax; opaque APIs and compiler checks establish actual
custody independently.

### Architecture lint input discovery

The harness move boundary checks parsed Rust paths and macro token trees.
Its positive lexical test exclusions use the same shared predicate as the
wire-width lane. Production following a test item, mixed test/production cfg,
qualified constructor paths and macro payloads remain checked; strings and
comments cannot become ownership escapes or exemptions.

The style lane checks `usize` wire fields in parsed structs and enum
variants, including nested collection and optional types. It respects each
serde direction's skipped fields and variants, excludes PhantomData's erased
type arguments, and shares the ownership visitors' positive test-cfg predicate.
An unguarded module named `tests` or a mixed test/production cfg does not exempt
its wire declarations.

Every explicit scan input must exist. In a Git checkout, architecture lints
inspect tracked and new untracked Rust sources, excluding ignored artifacts
from directory scans. An explicitly requested Rust file is scanned even when
ignored. Mixed valid/missing input cannot report a clean result. The real CLI
regressions in `tests/lint_input_discovery.rs` exercise each case in isolated Git
fixtures; full workspace test runs include this enforcement coverage.
A successful Git inventory with no Rust inputs fails explicitly; it does not
trigger a filesystem fallback that would reintroduce ignored artifacts. Outside
a Git checkout, native directory discovery remains available.

### Account creation submission guard

The frontend semantic handoff lint rejects actual parsed calls to
`submit_local_terminal_operation` or `LocalTerminalOperationOwner::submit`
whose direct operation-kind argument is `SemanticOperationKind::CreateAccount`.
The typed CreateAccount callback is the primary boundary; this guard prevents a
coordinated callback/signature change from restoring frontend-local completion.
Qualified paths and grouped arguments are covered without matching comments,
strings, or unrelated type names. Only proven lexical test predicates exclude
production enforcement; mixed predicates and later production items remain
checked. Syn fixtures cover rejection, sanctioned handoff, and lexical scopes.
Run `just ci-frontend-handoff-boundary` and the ownership aggregate when changing
this boundary.
