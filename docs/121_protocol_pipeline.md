# Protocol Pipeline (Multi-Party Protocols + Ceremonies)

This document defines the **required pipeline** for designing, implementing, and verifying Aura multi-party protocols.
It applies to all Layer 4/5 choreographies and all Category C ceremonies.

Constraints:
- Pre-launch: **no backwards compatibility**, **no migrations**, **zero legacy code**.
- Category C operations must follow the ceremony contract in `docs/118_key_rotation_ceremonies.md`.

## Scope
- Choreographies and protocols (MPST-based or equivalent)
- Ceremonies (Category C)
- Runtime wiring, state collection, and status reporting
- Tests and verification artifacts

## Required Artifacts (Pipeline)
1. **Operation classification (A/B/C)**
   - Source: `docs/117_operation_categories.md`
2. **Facts + reducers**
   - Fact types, schema versioning, view reducers, and status model
3. **Choreography spec**
   - MPST definition with roles, messages, and guards (`docs/107_mpst_and_choreography.md`)
4. **Runtime wiring**
   - `choreography_runtime.rs` (role runners), registration, guard chain integration
5. **Ceremony runner integration** (Category C)
   - Prestate binding, pending epoch, collect, commit/abort, supersession (`docs/118_key_rotation_ceremonies.md`)
6. **Status + reporting**
   - `CeremonyStatus` and protocol-specific status views
7. **Testing**
   - Shared bus integration tests
   - Simulation tests (partition / delay / failure)
   - Property tests when applicable
8. **Audit entry**
   - Update `docs/119_choreography_runtime_audit.md` status

## Review Gates
- **Design review**: classification, fact model, ceremony shape, choreography sketch
- **Implementation review**: runtime wiring + guard chain + status reporting
- **Verification review**: tests added and audit updated

## Protocol DoD / PR Checklist
- [ ] Operation category declared (A/B/C)
- [ ] Facts defined + reducer implemented + schema version bumped
- [ ] Choreography specified + roles/messages documented
- [ ] Runtime wiring added (role runners + registration)
- [ ] Category C uses ceremony runner and emits standard facts
- [ ] Status output implemented (`CeremonyStatus` or equivalent)
- [ ] Shared-bus integration test added
- [ ] Simulation test added
- [ ] Audit doc updated (`docs/119_choreography_runtime_audit.md`)

## Macro Scaffold
Use the `#[ceremony_facts]` macro from `aura-macros` to attach standard ceremony helpers
onto ceremony fact enums (canonical `ceremony_id()` + `ceremony_timestamp_ms()` accessors).

Example:
```rust
use aura_macros::ceremony_facts;

#[ceremony_facts]
pub enum InvitationFact {
    // ...domain variants...
    CeremonyInitiated { ceremony_id: CeremonyId, agreement_mode: Option<AgreementMode>, trace_id: Option<String>, timestamp_ms: u64 },
    CeremonyAcceptanceReceived { ceremony_id: CeremonyId, agreement_mode: Option<AgreementMode>, trace_id: Option<String>, timestamp_ms: u64 },
    CeremonyCommitted { ceremony_id: CeremonyId, relationship_id: String, agreement_mode: Option<AgreementMode>, trace_id: Option<String>, timestamp_ms: u64 },
    CeremonyAborted { ceremony_id: CeremonyId, reason: String, trace_id: Option<String>, timestamp_ms: u64 },
    CeremonySuperseded { superseded_ceremony_id: CeremonyId, superseding_ceremony_id: CeremonyId, reason: String, trace_id: Option<String>, timestamp_ms: u64 },
}
```

## Canonical Example
See consensus for the best-practice reference:
- `crates/aura-consensus/src/protocol/`
- `crates/aura-consensus/src/dkg/ceremony.rs`
