# 121 · BeeKEM Integration Strategy with OCAP Authorization

**Status:** Architecture Recommendation  
**Version:** 1.0  
**Created:** November 3, 2025  
**Supersedes:** 120_keyhive_integration.md

## Executive Summary

Following the critical review of our Keyhive integration design, this document proposes a revised architecture that:

1. **Extracts BeeKEM** as a standalone convergent TreeKEM implementation for group key agreement
2. **Implements OCAP tokens** to bridge threshold signatures with individual member authentication
3. **Integrates with Automerge** for CRDT-based state synchronization
4. **Separates concerns** between authorization (OCAP), encryption (BeeKEM), and state (Automerge)

This approach gives us exactly what we need: convergent MLS for eventually consistent environments, without adopting Keyhive's entire authorization model.

## Core Architecture

### 1. OCAP Token System (Authorization Layer)

The key insight is separating **authorization** (what can be done) from **authentication** (who can do it):

```rust
// Capability token: unforgeable right to perform an action
pub struct CapabilityToken {
    pub capability_id: CapabilityId,      // Unique identifier
    pub action: Action,                    // What this token permits
    pub resource: ResourceId,              // What resource it applies to
    pub constraints: Constraints,          // Time bounds, usage limits, etc.
    pub delegation_chain: Vec<Delegation>, // Parent capabilities
    pub threshold_proof: ThresholdProof,   // M-of-N signature
}

// Threshold proof: demonstrates M-of-N agreement
pub struct ThresholdProof {
    pub threshold: (u32, u32),              // (M, N)
    pub signers: Vec<DeviceId>,             // Which M devices signed
    pub aggregate_signature: ThresholdSig,   // FROST aggregate signature
}

// Individual authentication: device presents token to BeeKEM
pub struct IndividualAuth {
    pub device_id: DeviceId,
    pub capability_token: CapabilityToken,  // Proves authorization
    pub device_signature: Ed25519Signature, // Proves device control
}
```

**Key Properties:**
- **Threshold Creation**: M-of-N devices collaborate to create capability tokens
- **Individual Usage**: Any authorized device can use the token independently
- **Delegation**: Tokens can delegate to create sub-capabilities
- **Revocation**: Stored in Automerge CRDT for convergent revocation

### 2. BeeKEM Extraction Strategy

After analyzing Keyhive's implementation, BeeKEM has manageable dependencies that we can abstract:

#### Current BeeKEM Dependencies:
```rust
// From keyhive_core/src/cgka/beekem.rs
use crate::{
    crypto::{
        application_secret::PcsKey,      // Can abstract
        encrypted::EncryptedSecret,      // Can replace with our encryption
        share_key::{ShareKey, ShareSecretKey}, // Can adapt
        siv::Siv,                        // Standard SIV mode
    },
    principal::{
        document::id::DocumentId,        // Replace with our GroupId
        individual::id::IndividualId,    // Replace with our DeviceId
    },
};
```

#### Extraction Approach:

**Option A: Clean Extraction (Recommended)**
```rust
// Create crates/aura-beekem/ as standalone crate
pub mod aura_beekem {
    // Core BeeKEM logic with abstracted types
    pub trait BeeKemIdentity: Clone + Ord + Serialize {
        fn to_bytes(&self) -> Vec<u8>;
    }
    
    pub trait BeeKemCrypto {
        type PublicKey;
        type SecretKey;
        type EncryptedData;
        
        fn generate_keypair(&self) -> (Self::PublicKey, Self::SecretKey);
        fn encrypt(&self, key: &Self::SecretKey, data: &[u8]) -> Self::EncryptedData;
        fn decrypt(&self, key: &Self::SecretKey, data: &Self::EncryptedData) -> Vec<u8>;
    }
    
    pub struct BeeKem<I: BeeKemIdentity, C: BeeKemCrypto> {
        tree: BeeKemTree<I, C>,
        // Core convergent TreeKEM logic
    }
}
```

**Benefits of Extraction:**
- Clean separation from Keyhive's authorization model
- Can use Aura's existing crypto primitives
- Direct integration with Automerge for state sync
- Maintains BeeKEM's convergent properties

### 3. Automerge Integration

BeeKEM operations become Automerge CRDT operations:

```rust
// In Automerge document
pub enum CrdtOperation {
    // BeeKEM operations
    BeeKemAdd { member: DeviceId, public_key: PublicKey, path: PathChange },
    BeeKemUpdate { member: DeviceId, path: PathChange },
    BeeKemRemove { member: DeviceId },
    
    // OCAP operations
    CapabilityIssued { token: CapabilityToken },
    CapabilityRevoked { capability_id: CapabilityId, reason: String },
    CapabilityDelegated { parent: CapabilityId, child: CapabilityToken },
}

// Convergent merge semantics
impl MergeableOperation for CrdtOperation {
    fn merge(&self, other: &Self) -> MergeResult {
        match (self, other) {
            // Concurrent BeeKEM updates create multiple keys (per BeeKEM design)
            (BeeKemUpdate { .. }, BeeKemUpdate { .. }) => {
                MergeResult::Conflict(vec![self.clone(), other.clone()])
            }
            // Revocation wins over issuance
            (CapabilityIssued { token }, CapabilityRevoked { capability_id, .. }) 
                if token.capability_id == *capability_id => {
                MergeResult::Keep(other.clone())
            }
            // ... other merge rules
        }
    }
}
```

### 4. Integration Architecture

```
┌─────────────────────────────────────────────────────────┐
│                Application Layer                        │
│         (Group Messaging, Encrypted Storage)            │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│              OCAP Authorization Layer                   │
│                                                          │
│  • Threshold signatures create capability tokens        │
│  • Devices authenticate with tokens + signatures        │
│  • Delegation chains for fine-grained permissions       │
│  • Revocations stored in Automerge CRDT                │
└────────────────────┬────────────────────────────────────┘
                     │ Authorizes
┌────────────────────▼────────────────────────────────────┐
│         BeeKEM Group Key Agreement Layer                │
│                                                          │
│  • Convergent TreeKEM for eventual consistency          │
│  • Handles concurrent updates via multi-key nodes       │
│  • Forward secrecy + post-compromise security           │
│  • Members authenticated via OCAP tokens                │
└────────────────────┬────────────────────────────────────┘
                     │ Generates Keys
┌────────────────────▼────────────────────────────────────┐
│              Encryption Layer                           │
│                                                          │
│  • Application secrets from BeeKEM                      │
│  • Per-message encryption with derived keys             │
│  • Causal predecessor keys for CRDT access              │
└────────────────────┬────────────────────────────────────┘
                     │ State Sync
┌────────────────────▼────────────────────────────────────┐
│           Automerge CRDT State Layer                    │
│                                                          │
│  • BeeKEM operations as CRDT operations                 │
│  • Capability tokens and revocations                    │
│  • Convergent merge semantics                           │
│  • Causal ordering via Automerge                        │
└─────────────────────────────────────────────────────────┘
```

## Implementation Strategy

### Option 1: Extract BeeKEM (Recommended) ✅

**Pros:**
- Clean separation of concerns
- Can optimize for our use case
- Direct Automerge integration
- Maintains convergent properties
- We control the implementation

**Cons:**
- Need to maintain extracted code
- Must handle security audits ourselves
- Initial extraction effort

**Implementation Steps:**
1. Extract `cgka/beekem.rs` and dependencies to `crates/aura-beekem/`
2. Abstract Keyhive-specific types with traits
3. Implement trait adapters for Aura types
4. Add Automerge operation conversion
5. Security audit of extracted code

### Option 2: Use Keyhive with Adapter Layer ❌

**Pros:**
- No code extraction needed
- Keyhive maintains the implementation

**Cons:**
- Carries entire Keyhive dependency
- Complex adapter layer needed
- Impedance mismatch with our OCAP model
- Beelay sync conflicts with Automerge

**Not Recommended** due to architectural mismatch.

### Option 3: Reimplement BeeKEM 🤔

**Pros:**
- Complete control over implementation
- Can optimize specifically for Automerge
- No licensing concerns

**Cons:**
- High implementation risk
- Need extensive testing
- Requires cryptographic expertise
- Time-consuming

**Consider only if** extraction proves infeasible.

## Security Considerations

### BeeKEM Security Properties (Preserved)

1. **Forward Secrecy**: Key ratcheting ensures past keys cannot be recovered
2. **Post-Compromise Security**: Member removal prevents future access
3. **Concurrent Safety**: Multiple keys on conflict nodes maintain security

### OCAP Security Model

1. **Unforgeable Tokens**: Threshold signatures prevent single-device compromise
2. **Delegation Control**: Explicit delegation chains with constraints
3. **Revocation**: Convergent revocation via CRDT consensus
4. **Device Authentication**: Individual devices prove token possession

### Required Audits

Before production:
1. **BeeKEM Extraction**: Verify security properties preserved
2. **OCAP Implementation**: Formal verification of token semantics
3. **Automerge Integration**: Verify causal ordering maintained
4. **End-to-End**: Full protocol security audit

## Recommendation

**Extract BeeKEM as a standalone module** with the following approach:

1. **Create `aura-beekem` crate** with abstracted interfaces
2. **Implement OCAP token system** for threshold-to-individual bridge
3. **Integrate both with Automerge** for state synchronization
4. **Keep concerns separated**:
   - OCAP: Authorization and delegation
   - BeeKEM: Group key agreement
   - Automerge: State synchronization
   - Aura: Identity and threshold signatures

This gives us:
- ✅ Convergent MLS that works with eventual consistency
- ✅ Clean integration with our threshold identity model
- ✅ Direct Automerge CRDT integration
- ✅ Separation of authorization from key agreement
- ✅ No impedance mismatch with foreign authorization models

## Next Steps

1. **Prototype BeeKEM extraction** (1 week)
   - Extract core BeeKEM logic
   - Abstract type dependencies
   - Verify convergent properties preserved

2. **Design OCAP token schema** (1 week)
   - Token structure and constraints
   - Delegation chain semantics
   - Revocation propagation rules

3. **Implement Automerge operations** (1 week)
   - BeeKEM operation types
   - Merge semantics
   - Causal ordering verification

4. **Integration testing** (2 weeks)
   - Threshold → OCAP → BeeKEM flow
   - Concurrent update scenarios
   - Revocation cascade testing

5. **Security review** (ongoing)
   - Code audit of extraction
   - Formal verification planning
   - Threat modeling

## Conclusion

By extracting BeeKEM and implementing OCAP tokens, we get the best of both worlds:
- **Convergent group key agreement** from BeeKEM's innovative design
- **Threshold-compatible authorization** via OCAP tokens
- **CRDT-native integration** with Automerge
- **Clean architecture** with separated concerns

This approach is more aligned with Aura's architecture than trying to adopt Keyhive wholesale, and gives us exactly the convergent MLS capabilities we need for our eventually consistent system.

The key insight—using OCAP tokens to bridge threshold signatures with individual authentication—elegantly solves the identity mapping problem while maintaining security properties.

**Recommendation: Proceed with BeeKEM extraction and OCAP implementation.**