# Keyhive Usage Review for Aura

**Date:** 2025-11-03  
**Reviewer:** Claude Code  
**Keyhive Documentation Source:** deepwiki.com/inkandswitch/keyhive  
**Status:** Critical Review - Design Proposal vs Actual Implementation

## Executive Summary

After reviewing Keyhive's actual implementation via DeepWiki and comparing it with Aura's integration design document (`120_keyhive_integration.md`), I've identified several **critical architectural misalignments** between our proposed design and how Keyhive actually works. While the design document demonstrates strong understanding of Keyhive's purpose, the integration strategy contains fundamental misconceptions that would prevent successful implementation.

**Overall Assessment:** 🔴 **Critical Issues Found** - Design requires significant revision

## Current Implementation Status

**Status:** Design proposal only - no actual implementation yet

Evidence:
- Keyhive dependency declared in workspace `Cargo.toml` but not used
- No `use keyhive` imports found in any Aura crate
- Design document marked as "Status: Design Proposal, Target: Phase 3"
- Keyhive repository checked into `ext/keyhive/` but not integrated

**Implication:** We have time to correct the design before implementation begins.

## Critical Architectural Misalignments

### 🔴 Critical Issue #1: Misunderstanding of Keyhive's Authorization Model

**Our Design Assumes:**
> "Keyhive's convergent capabilities will completely replace Aura's biscuit-based policy engine"
> "Replace policy documents with capability delegation chains"

**Reality from Keyhive:**
Keyhive's "capabilities" are **NOT** the same as traditional capability-based security systems. Keyhive uses:
- **Delegations** signed by principals to grant access
- **Revocations** to remove access
- **Causal dependency graphs** of delegations/revocations
- **MemberAccess enum** with four hierarchical levels: `Pull`, `Read`, `Write`, `Admin`

**Key Misconception:**
The design conflates Keyhive's delegation system with capability-based access control (OCAP model). Keyhive's delegations are **relationship-based**, not capability-token-based:

1. **Keyhive delegations** are signed assertions about WHO (principal) can do WHAT (access level) on WHICH (group/document)
2. **Capability tokens** are unforgeable bearer tokens that grant authority to the holder

**Quote from DeepWiki:**
> "Access is delegated by creating `Delegation` objects, and revoked by creating `Revocation` objects"
> "Each `Delegation` includes a `proof` field, which is an optional reference to another `Delegation`, establishing a chain of authority"

**Impact:** 🔴 **Critical**
- The "convergent capabilities" terminology throughout the design is misleading
- Our proposed capability event schema doesn't match Keyhive's actual delegation model
- The capability-to-BeeKEM pipeline assumes a model that doesn't exist in Keyhive

### 🔴 Critical Issue #2: Misunderstanding BeeKEM's Role

**Our Design Assumes:**
> "BeeKEM for concurrent group key agreement"
> "Capability evaluations deterministically drive BeeKEM membership"

**Reality from Keyhive:**
BeeKEM is used **exclusively for document encryption**, not for general group membership or authorization:

**Quote from DeepWiki:**
> "Documents utilize CGKA (a variant of TreeKEM called BeeKEM) to maintain forward secrecy and post-compromise security"
> "The `Document` struct extends `Group` functionality with content encryption via CGKA"
> "Documents are encrypted using an `ApplicationSecret` derived from the current `PcsKey`"

**Key Points:**
1. **Groups** in Keyhive are authorization containers (delegations/revocations)
2. **Documents** are encrypted content containers that USE BeeKEM
3. BeeKEM membership is driven by document membership, not the other way around
4. Groups can exist without encryption (no BeeKEM)
5. Documents inherit group membership but add encryption layer

**Impact:** 🔴 **Critical**
- Our "Capability → BeeKEM Membership Pipeline" architecture is backwards
- The correct flow is: Delegation → Group Membership → Document Membership → BeeKEM
- We cannot use BeeKEM as a general CGKA for authorization

### 🔴 Critical Issue #3: Misunderstanding Keyhive's Synchronization Model

**Our Design Assumes:**
> "Capability mutations are encoded as CRDT events with explicit merge semantics"
> "Delegations and revocations become part of the unified CRDT history"

**Reality from Keyhive:**
Keyhive uses **Beelay** for synchronization, not a traditional CRDT:

**Quote from DeepWiki:**
> "Beelay Core implements the synchronization protocol, acting as a wrapper around Keyhive for network communication and storage management"
> "The synchronization process in Beelay involves three main phases: Membership Synchronization, Document Collection Synchronization, Individual Document Synchronization"
> "Peers exchange `MembershipSymbol`s generated from `StaticEvent`s representing membership operations using RIBLT to identify differences"

**Key Points:**
1. Keyhive uses **event-based state machine**, not CRDT operations
2. Synchronization happens through **RIBLT** (Reversible Invertible Bloom Lookup Tables), not CRDT merge
3. `StaticEvent`s are **serialized events**, not CRDT operations
4. Events are ingested in **epochs** with retry logic for out-of-order delivery
5. The system assumes **causal ordering**, which is ensured by Keyhive's event dependencies

**Impact:** 🔴 **Critical**
- Our CRDT event schema (`Event::CapabilityDelegation`, etc.) doesn't match Keyhive's `StaticEvent` system
- We cannot directly integrate Keyhive events into Automerge CRDT
- Need adapter layer to translate between Keyhive events and Aura's CRDT

### 🟡 Major Issue #4: Forward Secrecy Trade-off Mischaracterization

**Our Design States:**
> "The application-layer causal encryption deliberately sacrifices forward secrecy for CRDT functionality"
> "For collaborative documents, accessing historical states is often a feature"

**Reality from Keyhive:**
This characterization misses important nuances:

**Quote from DeepWiki:**
> "Documents utilize CGKA (a variant of TreeKEM called BeeKEM) to maintain forward secrecy and post-compromise security"
> "When a member performs an `update` operation, a new leaf key pair is generated, triggering a tree path update"

**Key Points:**
1. BeeKEM **does** provide forward secrecy through key ratcheting
2. The "causal encryption" mentioned is about **deriving ApplicationSecrets** with predecessor references
3. Forward secrecy is maintained at the CGKA level
4. Application-level encryption uses keys derived from PCS (Post-Compromise Security) keys

**Clarification Needed:**
The design implies we're "sacrificing forward secrecy for CRDT access" but Keyhive's actual model is more sophisticated:
- **CGKA layer**: Maintains forward secrecy via ratcheting
- **Application layer**: Derives keys that reference content predecessors (for causality)
- **Trade-off**: You can't decrypt historical content UNLESS you were a member when it was created

**Impact:** 🟡 **Major**
- Security properties are mischaracterized in the design
- Actual security model is better than described
- Need to update security section to reflect Keyhive's actual guarantees

### 🟡 Major Issue #5: Identity Integration Assumptions

**Our Design Assumes:**
> "Aura's threshold identity system signs capability delegations and revocations"
> "Use deterministic key derivation to generate context-specific keys"

**Reality from Keyhive:**
Keyhive has its own identity model that may not align cleanly with threshold signatures:

**Quote from DeepWiki:**
> "`Active<S,T,L>`: Represents the current user agent with signing capabilities"
> "`Individual`: Represents other users known to the system"
> "Users exchange `ContactCard`s to establish encrypted communication channels, which contain prekey operations"

**Key Points:**
1. Keyhive expects **individual signing keys**, not threshold signatures
2. The `Active` agent has its own key pair
3. Prekey exchange is designed for 1-to-1 or 1-to-many, not M-of-N threshold
4. `ContactCard`s encode prekey bundles for secure setup

**Concern:**
Threshold signatures work fundamentally differently from individual signing keys:
- **Threshold**: Multiple parties must coordinate to produce a single signature
- **Individual**: Single party produces signature with their private key

**Impact:** 🟡 **Major**
- May need to treat each Aura device as an "Individual" in Keyhive
- Threshold signatures may need to happen at a higher layer
- Need to carefully design how Aura's threshold identity maps to Keyhive's individual model

### 🟡 Major Issue #6: SBB Integration Overcomplexity

**Our Design Proposes:**
> "The Social Bulletin Board (SBB) system harmonizes with Keyhive's unified authorization model"
> "Replace simple counter coordination with capability-based envelope publishing rights"
> "Use convergent capabilities to grant and revoke SBB relay quotas dynamically"

**Concern:**
This adds significant complexity to SBB that may not be necessary:

1. **Keyhive is designed for documents and groups**, not bulletin boards
2. SBB requires different access patterns (publish-subscribe vs read-write)
3. Adding delegation chains to SBB relay decisions may introduce latency
4. The design tries to force-fit SBB into Keyhive's model

**Question:**
Is Keyhive the right tool for SBB authorization? Consider:
- SBB needs fast relay decisions (low latency)
- Keyhive delegations require signature verification and graph traversal
- SBB spam prevention needs different properties than document access control

**Impact:** 🟡 **Major**
- SBB integration may be over-engineered
- Consider simpler authorization for SBB separate from document access control
- May want to use Keyhive only for documents, not for relay coordination

## Architectural Recommendations

### Recommendation #1: Clarify Terminology

**Stop saying "capability-based" and "convergent capabilities".**

Use Keyhive's actual terminology:
- **Delegations** (not capability tokens)
- **Revocations** (explicit removal of delegations)
- **Authority graphs** (causal dependency of delegations)
- **MemberAccess levels** (Pull/Read/Write/Admin)

### Recommendation #2: Correct the Architecture Diagram

The correct Keyhive model is:

```
┌──────────────────────────────────────────────────┐
│          Application Layer (Aura Apps)           │
└────────────────────┬─────────────────────────────┘
                     │
┌────────────────────▼─────────────────────────────┐
│     Identity Layer (Aura Threshold Identity)     │
│  Maps to Keyhive "Individuals" (one per device)  │
└────────────────────┬─────────────────────────────┘
                     │
┌────────────────────▼─────────────────────────────┐
│          Keyhive Core (Authorization)            │
│                                                   │
│  Groups (Authorization Container)                │
│  ├─ Delegations (who can do what)               │
│  ├─ Revocations (remove access)                 │
│  └─ Authority Graph (delegation chains)         │
│                                                   │
│  Documents (Encrypted Content)                   │
│  ├─ Inherits Group membership                   │
│  ├─ BeeKEM for key agreement                    │
│  └─ ApplicationSecret for encryption            │
└────────────────────┬─────────────────────────────┘
                     │
┌────────────────────▼─────────────────────────────┐
│     Beelay Core (Synchronization)                │
│  ├─ StaticEvent propagation                     │
│  ├─ RIBLT-based difference detection            │
│  └─ Membership/Document sync phases             │
└────────────────────┬─────────────────────────────┘
                     │
┌────────────────────▼─────────────────────────────┐
│          Transport Layer (Aura P2P)              │
└──────────────────────────────────────────────────┘
```

**Key Changes:**
1. **Groups ≠ Documents**: Groups are authorization, Documents are encrypted content
2. **BeeKEM only for Documents**: Not for general group key agreement
3. **Beelay is required**: Cannot integrate Keyhive directly with Automerge CRDT
4. **Identity mapping**: Each Aura device → Keyhive Individual

### Recommendation #3: Revised Integration Strategy

**Phase 1: Understand Keyhive's Event Model**
- Study `StaticEvent` structure and serialization
- Understand `ingest_unsorted_static_events()` epoch-based processing
- Map Keyhive events to Aura's event system

**Phase 2: Identity Bridge Layer**
- Design how Aura devices map to Keyhive Individuals
- Handle threshold signature coordination for delegation signing
- Implement ContactCard exchange protocol

**Phase 3: Beelay Integration (Not Direct CRDT)**
- Use Beelay's synchronization protocol, don't bypass it
- Implement RIBLT-based sync for Keyhive events
- Bridge between Beelay sync and Aura's CRDT sync

**Phase 4: Document Encryption (Not General Authorization)**
- Use Keyhive Documents for encrypted content storage
- BeeKEM for group key agreement within documents
- Keep SBB authorization separate (simpler model)

**Phase 5: Authorization Layer (Delegations, Not Capabilities)**
- Implement delegation-based access control for documents
- Use authority graphs for revocation cascades
- Map to Aura's existing authorization needs

### Recommendation #4: Reconsider SBB Integration

**Question:** Does SBB need Keyhive?

Consider two-track approach:
1. **Document Access Control**: Use Keyhive delegations (high-value, high-latency acceptable)
2. **SBB Relay Permissions**: Use simpler token-based system (low-latency, spam prevention focus)

**Rationale:**
- SBB and document access have different requirements
- Forcing both into Keyhive may add unnecessary complexity
- Simpler SBB authorization easier to reason about

### Recommendation #5: Security Audit Requirements

Before implementation:
1. **Formal review of BeeKEM**: Ensure we understand its security properties
2. **Threshold signature compatibility**: Verify Keyhive works with M-of-N signing
3. **Event ordering guarantees**: Confirm causal ordering with distributed Aura devices
4. **Revocation cascade testing**: Extensive testing of delegation invalidation

## What We Got Right

Despite the critical issues, several aspects of the design are sound:

### ✅ Correct: Local-First Alignment
> "Keyhive is designed specifically for partition-tolerant, eventually consistent systems"

**Accurate.** Keyhive's event-based model and authority graphs are designed for local-first.

### ✅ Correct: Unified State Model Benefits
> "Single, eventually consistent authority graph manages authorization decisions and encryption access"

**Accurate.** Groups and Documents share the same delegation/revocation model.

### ✅ Correct: Forward Secrecy via BeeKEM
> "CGKA layer maintains forward secrecy through ratcheting"

**Accurate.** BeeKEM provides proper forward secrecy and post-compromise security.

### ✅ Correct: Phased Implementation
> "The integration represents a fundamental architectural shift requiring careful phased implementation"

**Accurate.** This is a major integration that needs careful staging.

## Critical Path Forward

### Immediate Actions (Before Implementation)

1. **Study Keyhive's Actual Codebase**
   - Read `keyhive_core/src/keyhive.rs` for the main API
   - Study delegation/revocation structures
   - Understand StaticEvent format

2. **Study Beelay's Sync Protocol**
   - Read `beelay-core` synchronization code
   - Understand RIBLT-based difference detection
   - Learn epoch-based event ingestion

3. **Prototype Identity Bridge**
   - Test threshold signatures with Keyhive
   - Verify each Aura device can be a Keyhive Individual
   - Validate ContactCard exchange

4. **Revise Design Document**
   - Remove "convergent capabilities" terminology
   - Correct BeeKEM role (documents only)
   - Fix synchronization model (Beelay, not direct CRDT)
   - Separate SBB from Keyhive integration

5. **Security Review**
   - Commission cryptographic audit of BeeKEM
   - Verify threshold signature compatibility
   - Test revocation cascade properties

### Updated Risk Assessment

**Before Review:**
- ❓ Unknown risks due to novel integration

**After Review:**
- 🔴 **High Risk**: Current design would fail to integrate due to architectural misalignments
- 🟡 **Medium Risk**: Identity mapping (threshold ↔ individual) needs validation
- 🟡 **Medium Risk**: Synchronization requires Beelay, not direct CRDT integration
- 🟢 **Low Risk**: BeeKEM security properties are sound (with audit)

**Mitigation:**
Design document requires **major revision** before implementation begins.

## Conclusion

Aura's Keyhive integration design demonstrates strong motivation and understanding of **why** we want Keyhive, but contains critical misunderstandings of **how** Keyhive actually works. The good news is that we caught this during the design phase, before implementation began.

### Summary of Critical Issues

1. 🔴 **Terminology**: "Convergent capabilities" ≠ Keyhive delegations
2. 🔴 **Architecture**: BeeKEM is for documents only, not general group key agreement
3. 🔴 **Synchronization**: Must use Beelay, cannot integrate directly with Automerge CRDT
4. 🟡 **Identity**: Threshold signatures may not map cleanly to Keyhive's individual model
5. 🟡 **SBB**: Over-engineered integration that may not be necessary

### Recommended Next Steps

**DO NOT BEGIN IMPLEMENTATION** using the current design document.

**Priority 1: Education**
- Deep dive into Keyhive codebase (`ext/keyhive/keyhive_core/`)
- Study Beelay synchronization protocol
- Build proof-of-concept prototypes

**Priority 2: Design Revision**
- Rewrite architecture sections with correct understanding
- Separate document encryption (Keyhive) from relay permissions (simpler)
- Design proper Beelay integration strategy

**Priority 3: Validation**
- Prototype threshold signature ↔ Keyhive Individual bridge
- Test event synchronization between Keyhive and Aura
- Validate security properties with cryptographic expert

**Priority 4: Staged Implementation**
- Start with simple document encryption use case
- Add delegation-based authorization incrementally
- Keep SBB separate until document access control is proven

### Final Assessment

**Current Design Status:** 🔴 **Not Ready for Implementation**

**Path Forward:** ✅ **Revise design with correct Keyhive understanding, then proceed**

The Keyhive integration is still a promising direction for Aura, but requires significant design correction before implementation can begin safely.

## References

- **Keyhive Repository:** `ext/keyhive/`
- **Keyhive Documentation:** https://deepwiki.com/inkandswitch/keyhive
- **Aura Design Document:** `docs/120_keyhive_integration.md`
- **Keyhive Core API:** `ext/keyhive/keyhive_core/src/keyhive.rs`
- **Beelay Sync Protocol:** Documented via DeepWiki queries
- **BeeKEM Implementation:** `ext/keyhive/keyhive_core/src/cgka.rs`
