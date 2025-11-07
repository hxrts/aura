# ID Types

## Summary

This document describes the standardization and consolidation of ID type definitions across the Aura codebase. The initiative addresses redundancy and inconsistency in identifier types by establishing **aura-types** as the single source of truth for all core ID definitions.

## Problem Statement

### Observations

1. **Redundant Definitions**: Numerous ID-like types were defined in multiple locations with similar implementations:
   - `EventId` (aura-types vs journal/protocols/events.rs)
   - `CapabilityId` (aura-types vs journal/capability/types.rs)
   - `MemberId` (aura-types vs journal/capability/group_capabilities.rs)
   - `IndividualId` (aura-types vs journal/capability/identity.rs)
   - `OperationId` (aura-types vs journal/capability/group_capabilities.rs)
   - `ParticipantId` (3 different definitions across aura-types, journal, and simulator)

2. **Inconsistent Patterns**: ID type implementations varied in:
   - Constructor patterns (`new()`, `new_with_effects()`, `from_*()`)
   - Display formats (prefixed vs raw)
   - Conversion implementations
   - Effect-based generation support

3. **Boilerplate**: Repeated patterns for:
   - UUID-based IDs with `new()` and `from_uuid()`
   - String-based IDs with `Into<String>` constructors
   - Byte array IDs with hex conversions
   - Serialization/deserialization support

## Solution Overview

### 1. Centralized ID Definitions in aura-types

All core ID types are now defined exclusively in **aura-types/src/identifiers.rs** and related modules:

- **UUID-based IDs**: SessionId, EventId, OperationId, DeviceId, GuardianId, AccountId
- **String-based IDs**: MemberId, IndividualId, ContextId, Cid
- **Numeric IDs**: EventNonce, Epoch, ContentSize
- **Byte-array IDs**: ChunkId, CapabilityId, RelationshipId

### 2. Standardized Constructor Patterns

#### UUID-based IDs

```rust
impl SessionId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
    pub fn from_uuid(uuid: Uuid) -> Self { Self(uuid) }
    pub fn uuid(&self) -> Uuid { self.0 }
}

impl Default for SessionId {
    fn default() -> Self { Self::new() }
}
```

#### String-based IDs

```rust
impl MemberId {
    pub fn new(id: impl Into<String>) -> Self { Self(id.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

#### Effects-aware Extensions

Extension traits enable deterministic ID generation for testing:

```rust
pub trait EventIdExt {
    fn new_with_effects(effects: &impl EffectsLike) -> Self;
}

impl EventIdExt for EventId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        EventId(effects.gen_uuid())
    }
}
```

### 3. Unified Display Formats

Consistent Display implementations across all ID types:

**Prefixed Format (for user-facing output)**:
- SessionId: `"session-{uuid}"`
- EventId: `"event-{uuid}"`
- MemberId: `"member-{string}"`
- ChunkId: `"chunk-{hex}"`
- CapabilityId: `"capability-{hex}"`
- RelationshipId: `"relationship-{hex}"`

**Raw Format (for system IDs)**:
- DeviceId, GuardianId, AccountId: `"{uuid}"` (no prefix)
- ContextId: `"context:{string}"`
- Epoch: `"epoch-{u64}"`

### 4. ID Type Generation Macros

New macro system in **aura-types/src/macros.rs** reduces boilerplate:

```rust
// UUID-based ID with prefix
define_uuid_id!(SessionId, "session");

// UUID-based ID without prefix
define_uuid_id!(DeviceId);

// String-based ID with prefix
define_string_id!(MemberId, "member");

// Numeric ID with prefix
define_numeric_id!(EventNonce, u64, "nonce");
```

### 5. ParticipantId Consolidation

**Three separate ParticipantId definitions resolved**:

1. **aura-types/sessions.rs** (authoritative): General participant enum
   ```rust
   pub enum ParticipantId {
       Device(DeviceId),
       Guardian(GuardianId),
   }
   ```
   - Purpose: Identify protocol participants (devices or guardians)
   - Used for: Session types, distributed protocols

2. **journal/capability/threshold_capabilities.rs** (renamed): Threshold-specific index
   ```rust
   pub struct ThresholdParticipantId(NonZeroU16);
   ```
   - Purpose: Index participants in threshold signature schemes
   - Used for: Threshold cryptography, capability delegation
   - Old name `ParticipantId` deprecated with alias for backward compatibility

3. **simulator/engine/types.rs** (kept separate): Simulation-specific identifier
   ```rust
   pub struct ParticipantId(pub Uuid);
   ```
   - Purpose: Identify simulated entities
   - Used for: Simulation engine, testing framework
   - No conflict because used only in simulator crate

## Changes Made

### 1. Removed Duplicate Definitions

- **journal/protocols/events.rs**: Removed duplicate `EventId`, now imports from aura-types
- **journal/capability/types.rs**: Removed duplicate `CapabilityId`, now uses aura-types with `from_chain()` extension
- **journal/capability/group_capabilities.rs**: Removed duplicate `MemberId` and `OperationId`
- **journal/capability/identity.rs**: Removed duplicate `IndividualId`
- **journal/capability/threshold_capabilities.rs**: Renamed `ParticipantId` to `ThresholdParticipantId`

### 2. Added Extensions to aura-types

#### EventIdExt
- `new_with_effects()`: Create EventId using injected effects

#### IndividualIdExt
- `from_device()`: Create from DeviceId
- `from_dkd_context()`: Create from DKD context and fingerprint

#### CapabilityId
- `from_chain()`: Deterministic derivation from parent chain

#### IndividualIdCapabilityExt (in journal)
- `to_subject()`: Convert to capability Subject

### 3. Updated Imports

All crates now import ID types from aura-types:

```rust
// Before (scattered)
use journal::capability::types::{CapabilityId, Subject};
use journal::capability::identity::IndividualId;
use journal::capability::group_capabilities::MemberId;

// After (unified)
use aura_types::{CapabilityId, IndividualId, MemberId};
```

### 4. Added Macros Module

New **aura-types/src/macros.rs** provides three macros:

- `define_uuid_id!`: Generate UUID-based ID types
- `define_string_id!`: Generate String-based ID types
- `define_numeric_id!`: Generate numeric ID types

Each macro generates:
- Struct definition with appropriate derives
- Constructor methods
- Display implementation
- From conversions
- Documentation

## Benefits

### Code Reduction
- Eliminated ~200 lines of duplicate boilerplate
- Simplified imports across journal crate
- Centralized maintenance point

### Type Safety
- Single canonical definition prevents inconsistencies
- Extension traits provide opt-in additional functionality
- Macros ensure consistent implementations

### Developer Experience
- Clear patterns for adding new ID types
- Automatic documentation generation via macros
- Centralized place to understand ID type conventions

### Testability
- Effect-based ID generation for deterministic testing
- Extension traits for specialized constructors
- Consistent conversion patterns

## Migration Path

### For New ID Types

Use the appropriate macro in aura-types:

```rust
// In aura-types/src/new_module.rs
define_uuid_id!(NewId, "prefix");

// Or
define_string_id!(ConfigId);

// Then export from lib.rs
pub use new_module::*;
```

### For Specialized Behavior

Create extension traits:

```rust
pub trait NewIdExt {
    fn from_context(context: &str) -> Self;
}

impl NewIdExt for NewId {
    fn from_context(context: &str) -> Self {
        // Implementation
    }
}
```

### For Existing Code

Gradually migrate to aura-types versions:

1. Replace local imports with aura-types imports
2. Update re-exports in module files
3. Fix trait import issues (use extension traits)
4. Update any specialized behavior

## Current Status

### Completed

- ✅ Removed duplicate EventId from journal
- ✅ Consolidated CapabilityId with `from_chain()` support
- ✅ Centralized MemberId, IndividualId, OperationId
- ✅ Resolved ParticipantId conflicts (renamed to ThresholdParticipantId)
- ✅ Created ID type generation macros
- ✅ Standardized Display formats
- ✅ Added Effects-aware extensions
- ✅ Updated all imports in journal crate
- ✅ Verified builds with no duplicate definitions

### Ongoing

- Migrate simulator's ParticipantId to use aura-types enum (when appropriate)
- Update any remaining crates using journal-specific ID types
- Document macro usage in development guidelines

### Future

- Consider macro expansion for other type patterns
- Establish broader standardization initiative for other types
- Update CLAUDE.md with new patterns
- Create code generation templates for common patterns

## Testing

All changes have been validated:

- `cargo build -p aura-types`: ✅ Builds successfully
- `cargo build -p aura-journal`: ✅ Builds successfully with no duplicate definitions
- Deprecated warnings show ThresholdParticipantId migration path
- Extension traits properly scoped for use where needed

## Documentation

### For Developers

When adding a new ID type:

1. Define in appropriate module within aura-types
2. Use `define_uuid_id!`, `define_string_id!`, or `define_numeric_id!` macro
3. Add extension traits for specialized constructors (if needed)
4. Export from lib.rs
5. Update this document with new type reference

### For Users of ID Types

1. Import from aura-types (not local modules)
2. Use `new()` for random generation
3. Use `new_with_effects()` for deterministic testing
4. Use specialized constructors via extension traits (e.g., `from_device()`)
5. Use Display implementation for user-facing output

## References

- aura-types/src/identifiers.rs: Core UUID and String ID definitions
- aura-types/src/macros.rs: ID type generation macros
- aura-types/src/capabilities.rs: CapabilityId definition
- aura-types/src/relationships.rs: RelationshipId definition
- journal/src/capability/threshold_capabilities.rs: ThresholdParticipantId definition
