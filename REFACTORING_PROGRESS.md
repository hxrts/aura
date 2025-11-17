# Architecture Refactoring Progress

## In Progress

### Phase 3: Layer 4 Violations (1 task remaining)

#### 9. Move time_enhanced.rs
**Priority**: Medium
**Status**: The file `time_enhanced.rs` still exists in `crates/aura-protocol/src/handlers/`. It should be moved to `aura-effects`.

---

## Pending Tasks 📋

### Phase 4: Layer 2 Violations (2 tasks remaining)

#### 11-12. Move aura-mpst handlers to aura-protocol
**Priority**: High
**Effort**: 16-24 hours
**Files**:
- `crates/aura-mpst/src/runtime.rs` (AuraHandler, AuraRuntime) → `crates/aura-protocol/src/handlers/`
- `crates/aura-mpst/src/journal.rs` (effect execution) → `crates/aura-protocol/src/`
**Status**: Files and logic remain in `aura-mpst`.

### Phase 5: Layer 1 Violations (5 tasks - MOST COMPLEX)

#### 14. Create aura-context crate and move context_derivation.rs
**Priority**: High
**Effort**: 12-16 hours
**Files**: `crates/aura-core/src/context_derivation.rs` (520 lines) → new `crates/aura-context/`
**Status**: `aura-context` crate does not exist.

#### 15. Split journal.rs
**Priority**: High
**Effort**: 24-32 hours
**Files**: `crates/aura-core/src/journal.rs` (1,524 lines) → split into:
- Types → stay in aura-core
- CRDT logic → aura-journal
- Authorization → aura-verify
**Status**: File is still over 1000 lines and contains significant CRDT and Authorization logic.

#### 16. Move causal_context.rs to aura-journal
**Priority**: High
**Effort**: 8-12 hours
**Files**: `crates/aura-core/src/causal_context.rs` (307 lines)
**Status**: File still exists in `aura-core`.

#### 17. Move flow.rs to aura-verify
**Priority**: High
**Effort**: 8-12 hours
**Files**: `crates/aura-core/src/flow.rs` (257 lines)
**Status**: File still exists in `aura-core`.

#### 18. Move crypto/key_derivation.rs to aura-effects
**Priority**: Medium
**Effort**: 4-8 hours
**Files**: `crates/aura-core/src/crypto/key_derivation.rs`
**Status**: File still exists in `aura-core`.

### Phase 6: DRY Improvements (5 tasks)

#### 19. Consolidate error handling
**Priority**: Medium
**Effort**: 8-16 hours
**Impact**: Eliminate 150+ lines of duplication

#### 20. Unify retry logic
**Priority**: Medium
**Effort**: 16-24 hours
**Impact**: Eliminate 400+ lines of duplication

#### 21. Create unified builder pattern utility
**Priority**: Low
**Effort**: 8-16 hours
**Impact**: Eliminate 300+ lines of boilerplate

#### 22. Consolidate test fixtures in aura-testkit
**Priority**: Low
**Effort**: 8-12 hours
**Impact**: Eliminate 400+ lines of duplication

#### 23. Create generic handler adapter pattern
**Priority**: Low
**Effort**: 4-8 hours
**Impact**: Eliminate 200+ lines of boilerplate

---

## Estimated Remaining Effort

### By Phase
- **Phase 3** (remaining): 8-12 hours
- **Phase 4**: 24-36 hours
- **Phase 5**: 56-80 hours (most complex)
- **Phase 6**: 44-76 hours

**Total Remaining**: 132-204 hours (16.5-25.5 engineering days)

### By Priority
- **Critical**: 48-72 hours (Phase 4-5)
- **High**: 36-56 hours (error/retry consolidation, handlers)
- **Medium**: 24-40 hours (DRY improvements)

---

## Recommendations

### Immediate Next Steps

1. **Complete Phase 3 Item #9** (time_enhanced.rs migration) - High Priority
2. **Address Phase 4** (Layer 2 violations) - Critical Priority
3. **Begin Phase 5** (Layer 1 violations) - Critical Priority
