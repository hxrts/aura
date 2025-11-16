# Architecture Refactoring Progress

**Date Started**: 2025-11-16
**Branch**: `claude/review-project-docs-01BRkUeHWpx79mSHxJT9vmMJ`

---

## Progress Summary

**Overall**: 2 of 23 tasks completed (9%)
**Status**: Phase 1 (Quick Wins) in progress

---

## Completed Tasks ✅

### Phase 1: Quick Wins (2/3 complete)

#### ✅ 1. Delete unused DKD message types
**Commit**: 720387e
**Files Changed**:
- Deleted: `crates/aura-protocol/src/messages/crypto/dkd.rs` (98 lines)
- Modified: `crates/aura-protocol/src/messages/crypto/mod.rs`

**Impact**: Removed 98 lines of dead code that was defined but never used. Cleaned up CryptoPayload enum and added documentation indicating DKD will be in a future aura-dkd feature crate.

#### ✅ 2. Remove MockJournalHandler re-export
**Commit**: 720387e
**Files Changed**:
- Modified: `crates/aura-protocol/src/handlers/mod.rs` (line 482)

**Impact**: Removed incorrect layer crossing where aura-protocol was re-exporting aura-effects types. Users should import MockJournalHandler directly from aura-effects. Verified no code was using this re-export.

---

## In Progress 🔄

### Phase 1: Quick Wins (1 remaining)

#### 🔄 3. Move CLI effects from aura-cli to aura-protocol
**Status**: Started but not completed
**Complexity**: Moderate - requires careful import updates across crates
**Files to Move**:
- `crates/aura-cli/src/effects/mod.rs` → `crates/aura-protocol/src/effects/cli/mod.rs`
- `crates/aura-cli/src/effects/cli.rs` → `crates/aura-protocol/src/effects/cli/handler.rs`
- `crates/aura-cli/src/effects/output.rs` → `crates/aura-protocol/src/effects/cli/output.rs`
- `crates/aura-cli/src/effects/config.rs` → `crates/aura-protocol/src/effects/cli/config.rs`

**Next Steps**:
1. Fix imports in moved files (replace `crate::effects::Result` with `aura_core::AuraResult`)
2. Update module exports in aura-protocol
3. Update aura-cli to import from aura-protocol
4. Remove effects/ directory from aura-cli
5. Verify compilation

---

## Pending Tasks 📋

### Phase 2: Layer 3 Violations (2 tasks)

#### 4. Move TransportCoordinator to aura-protocol
**Priority**: Critical
**Effort**: 4-6 hours
**Files**: `crates/aura-effects/src/transport/coordination.rs` → `crates/aura-protocol/src/handlers/`

#### 5. Move RealTimeHandler coordination to aura-protocol
**Priority**: Critical
**Effort**: 4-6 hours
**Files**: `crates/aura-effects/src/time.rs` (RealTimeHandler) → `crates/aura-protocol/src/handlers/`

### Phase 3: Layer 4 Violations (5 tasks)

#### 6-9. Move system handlers to aura-effects
**Priority**: High
**Effort**: 8-12 hours total
**Files**:
- `crates/aura-protocol/src/handlers/system/logging.rs`
- `crates/aura-protocol/src/handlers/system/metrics.rs`
- `crates/aura-protocol/src/handlers/system/monitoring.rs`
- `crates/aura-protocol/src/handlers/time_enhanced.rs`

#### 10. Move agent effect traits to aura-core
**Priority**: High
**Effort**: 4-6 hours
**Files**: `crates/aura-protocol/src/effects/agent.rs` → `crates/aura-core/src/effects/`

### Phase 4: Layer 2 Violations (3 tasks)

#### 11-12. Move aura-mpst handlers to aura-protocol
**Priority**: Critical
**Effort**: 16-24 hours
**Files**:
- `crates/aura-mpst/src/runtime.rs` (AuraHandler, AuraRuntime) → `crates/aura-protocol/src/handlers/`
- `crates/aura-mpst/src/journal.rs` (effect execution) → `crates/aura-protocol/src/`

#### 13. Refactor aura-wot CapabilityEvaluator
**Priority**: Critical
**Effort**: 8-12 hours
**Files**: `crates/aura-wot/src/capability_evaluator.rs`

### Phase 5: Layer 1 Violations (5 tasks - MOST COMPLEX)

#### 14. Create aura-context crate and move context_derivation.rs
**Priority**: Critical
**Effort**: 12-16 hours
**Files**: `crates/aura-core/src/context_derivation.rs` (520 lines) → new `crates/aura-context/`

#### 15. Split journal.rs
**Priority**: Critical
**Effort**: 24-32 hours
**Files**: `crates/aura-core/src/journal.rs` (1,524 lines) → split into:
- Types → stay in aura-core
- CRDT logic → aura-journal
- Authorization → aura-verify

#### 16. Move causal_context.rs to aura-journal
**Priority**: Critical
**Effort**: 8-12 hours
**Files**: `crates/aura-core/src/causal_context.rs` (307 lines)

#### 17. Move flow.rs to aura-verify
**Priority**: Critical
**Effort**: 8-12 hours
**Files**: `crates/aura-core/src/flow.rs` (257 lines)

#### 18. Move crypto/key_derivation.rs to aura-effects
**Priority**: High
**Effort**: 4-8 hours
**Files**: `crates/aura-core/src/crypto/key_derivation.rs`

### Phase 6: DRY Improvements (5 tasks)

#### 19. Consolidate error handling
**Priority**: High
**Effort**: 8-16 hours
**Impact**: Eliminate 150+ lines of duplication

#### 20. Unify retry logic
**Priority**: High
**Effort**: 16-24 hours
**Impact**: Eliminate 400+ lines of duplication

#### 21. Create unified builder pattern utility
**Priority**: Medium
**Effort**: 8-16 hours
**Impact**: Eliminate 300+ lines of boilerplate

#### 22. Consolidate test fixtures in aura-testkit
**Priority**: Medium
**Effort**: 8-12 hours
**Impact**: Eliminate 400+ lines of duplication

#### 23. Create generic handler adapter pattern
**Priority**: Medium
**Effort**: 4-8 hours
**Impact**: Eliminate 200+ lines of boilerplate

---

## Estimated Remaining Effort

### By Phase
- **Phase 1** (remaining): 4-6 hours
- **Phase 2**: 8-12 hours
- **Phase 3**: 12-18 hours
- **Phase 4**: 24-36 hours
- **Phase 5**: 56-80 hours (most complex)
- **Phase 6**: 44-76 hours

**Total Remaining**: 148-228 hours (18.5-28.5 engineering days)

### By Priority
- **Critical**: 88-132 hours (Phase 1-5)
- **High**: 36-56 hours (error/retry consolidation, handlers)
- **Medium**: 24-40 hours (DRY improvements)

---

## Recommendations

### Immediate Next Steps

1. **Complete Phase 1 Item #3** (CLI effects migration) - 4-6 hours
   - This completes the quick wins phase
   - Relatively isolated change with clear benefits

2. **Tackle Phase 2** (Layer 3 violations) - 8-12 hours
   - Move TransportCoordinator and RealTimeHandler
   - Clear architectural violations
   - Moderate complexity, good momentum builder

3. **Address Phase 3** (Layer 4 violations) - 12-18 hours
   - Move system handlers and agent traits
   - Multiple small, independent changes
   - Can be done incrementally

### Strategic Approach

- **Short-term (1-2 weeks)**: Complete Phases 1-3 (24-36 hours)
  - Builds momentum with quick wins
  - Addresses clear layer violations
  - Each task is relatively independent

- **Medium-term (3-4 weeks)**: Address Phase 4 (24-36 hours)
  - More complex refactoring (aura-mpst, aura-wot)
  - Requires careful dependency management
  - Critical for architectural integrity

- **Long-term (5-8 weeks)**: Tackle Phase 5 (56-80 hours)
  - Most complex: splitting aura-core
  - Requires creating new crates
  - Highest impact on architecture
  - Should be done last when patterns are established

- **Ongoing**: Implement Phase 6 DRY improvements (44-76 hours)
  - Can be done in parallel with other phases
  - High-priority items (error/retry) should be done early
  - Medium-priority items can be deferred

### Success Metrics

- ✅ Zero circular dependencies maintained
- ✅ Each layer has clear responsibilities
- ✅ Foundation layer (aura-core) contains only types and traits
- ✅ ~3,000 lines of duplicate code eliminated
- ✅ All 23 architecture violations resolved

---

## Notes

- All work is on branch `claude/review-project-docs-01BRkUeHWpx79mSHxJT9vmMJ`
- Each phase completion should be committed with clear messages
- Comprehensive testing after each major refactoring
- Architecture review findings document updated as tasks complete

**Last Updated**: 2025-11-16
