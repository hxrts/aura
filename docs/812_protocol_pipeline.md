# Protocol Pipeline

This document defines the required pipeline for designing, implementing, and verifying Aura multi-party protocols. It applies to all Layer 4/5 choreographies and all Category C ceremonies.

## Scope

The pipeline covers choreographies and protocols (MPST-based or equivalent), ceremonies (Category C), runtime wiring with state collection and status reporting, and tests with verification artifacts.

## Required Artifacts

### 1. Operation Classification

Classify the operation as Category A, B, or C using the decision tree in [Operation Categories](107_operation_categories.md).

### 2. Facts and Reducers

Define fact types with schema versioning. Implement view reducers. Define the status model for the operation category.

### 3. Choreography Specification

Create an MPST definition with roles, messages, and guards. See [MPST and Choreography](108_mpst_and_choreography.md) for the DSL and projection rules.

### 4. Runtime Wiring

Implement role runners in the choreography runtime. Register the protocol with the runtime. Integrate with the guard chain (CapGuard → FlowGuard → JournalCoupler).

### 5. Ceremony Runner Integration (Category C)

Category C operations must follow the ceremony contract in [Operation Categories](107_operation_categories.md). This includes prestate binding, pending epoch management, response collection, and commit/abort semantics with supersession handling.

### 6. Status and Reporting

Implement `CeremonyStatus` (for Category C) or protocol-specific status views. Ensure status is queryable for UI consumption.

### 7. Testing

Add shared bus integration tests. Add simulation tests covering partitions, delays, and failures. Add property tests when applicable.

## Review Gates

- **Design review**: Classification, fact model, ceremony shape, choreography sketch
- **Implementation review**: Runtime wiring, guard chain integration, status reporting
- **Verification review**: Tests added and passing

## Protocol Definition of Done

- [ ] Operation category declared (A/B/C)
- [ ] Facts defined with reducer and schema version
- [ ] Choreography specified with roles/messages documented
- [ ] Runtime wiring added (role runners + registration)
- [ ] Category C uses ceremony runner and emits standard facts
- [ ] Status output implemented
- [ ] Shared-bus integration test added
- [ ] Simulation test added

## Ceremony Facts Macro

Use the `#[ceremony_facts]` macro from `aura-macros` to attach standard ceremony helpers onto ceremony fact enums:

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
    CeremonyAcceptanceReceived {
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
    CeremonySuperseded {
        superseded_ceremony_id: CeremonyId,
        superseding_ceremony_id: CeremonyId,
        reason: String,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
}
```

The macro provides canonical `ceremony_id()` and `ceremony_timestamp_ms()` accessors for all ceremony fact variants.

## Canonical Example

See the consensus implementation for best-practice reference:

- `crates/aura-consensus/src/protocol/`
- `crates/aura-consensus/src/dkg/ceremony.rs`

## See Also

- [Operation Categories](107_operation_categories.md) for classification and ceremony contract
- [MPST and Choreography](108_mpst_and_choreography.md) for choreography DSL and runtime
- [Development Patterns](805_development_patterns.md) for general development workflows
