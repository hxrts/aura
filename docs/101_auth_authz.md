# 101 · Authentication vs Authorization Architecture

**Purpose**: Canonical explanation of Aura's authentication and authorization layer separation.

**Key Insight**: Authentication (WHO) and Authorization (WHAT) are cleanly separated with well-defined integration patterns.

## Architecture Overview

Aura maintains strict architectural separation between authentication and authorization with clean integration:

- **Authentication Layer**: [`crates/aura-verify/`](../crates/aura-verify/) + [`crates/aura-authenticate/`](../crates/aura-authenticate/) - Identity verification
- **Authorization Layer**: [`crates/aura-wot/`](../crates/aura-wot/) - Capability-based access control  
- **Integration**: [`crates/aura-protocol/src/authorization_bridge.rs`](../crates/aura-protocol/src/authorization_bridge.rs) - Clean composition

**See**: [`docs/099_glossary.md`](099_glossary.md) for complete terminology reference.

---

## Authentication Layer (WHO)

### aura-verify: Pure Cryptographic Identity Verification

**Location**: [`crates/aura-verify/`](../crates/aura-verify/)

**Responsibilities**:
- Device signature verification - proves a specific device signed a message
- Guardian signature verification - proves a guardian authorized an operation  
- Threshold signature verification - proves M-of-N parties signed collectively
- Session ticket verification - proves valid authenticated session

**Key Types**:
```rust
pub enum IdentityProof {
    Device { device_id: DeviceId, signature: Ed25519Signature },
    Guardian { guardian_id: GuardianId, signature: Ed25519Signature },
    Threshold(ThresholdSig),
}

pub struct VerifiedIdentity {
    pub proof_type: IdentityProofType,
    pub device_id: Option<DeviceId>,
    pub guardian_id: Option<GuardianId>,
    pub threshold_participants: Option<BTreeSet<DeviceId>>,
}

// Core verification function
pub fn verify_identity_proof(
    proof: &IdentityProof,
    message: &[u8], 
    key_material: &KeyMaterial,
) -> Result<VerifiedIdentity, VerificationError>
```

**Principle**: Stateless cryptographic verification. No policy knowledge, no authorization context.

### aura-authenticate: Choreographic Authentication Protocols  

**Location**: [`crates/aura-authenticate/`](../crates/aura-authenticate/)

**Responsibilities**:
- Device authentication ceremonies using Multi-Party Session Types
- Session establishment protocols with distributed coordination
- Guardian authentication flows for recovery operations
- Choreographic protocol definitions for distributed authentication

**Key Components**:
```rust
// Choreographic authentication protocol
choreography! {
    protocol G_auth {
        roles: Requester, Authenticator, Witness;
        Requester -> Authenticator: AuthRequest(device_id, challenge);
        Authenticator -> Requester: AuthResponse(signature, session_ticket);
        // ...
    }
}

pub struct AuthenticationResult {
    pub verified_identity: VerifiedIdentity,
    pub session_ticket: SessionTicket,
    pub ceremony_transcript: Vec<AuthEvent>,
}
```

**Dependencies**: Uses `aura-verify` for cryptographic verification, adds choreographic coordination.

---

## Authorization Layer (WHAT)

### aura-wot: Capability-Based Authorization

**Location**: [`crates/aura-wot/`](../crates/aura-wot/)

**Responsibilities**:
- Meet-semilattice capability operations (can only shrink via ⊓)
- Policy enforcement and evaluation for tree operations
- Delegation chains with proper capability attenuation  
- Web-of-trust relationship evaluation
- Storage access control based on capabilities

**Key Types**:
```rust
pub struct CapabilitySet {
    capabilities: BTreeSet<Capability>,
}

impl CapabilitySet {
    // Meet-semilattice operation (intersection only)
    pub fn meet(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            capabilities: self.capabilities.intersection(&other.capabilities).cloned().collect()
        }
    }
}

pub fn evaluate_tree_operation_capabilities(
    operation: &TreeOp,
    context: &AuthorizationContext,
    policies: &PolicySet,
) -> Result<PermissionGrant, AuthorizationError>
```

**Formal Properties**: Meet-semilattice laws verified through property-based tests:
- **Associativity**: `a.meet(b.meet(c)) == a.meet(b).meet(c)`
- **Commutativity**: `a.meet(b) == b.meet(a)`  
- **Idempotence**: `a.meet(a) == a`
- **Monotonicity**: Result is always subset of both inputs

**Principle**: Pure capability evaluation. No identity verification, no cryptographic operations.

---

## Integration: Authorization Bridge

### Clean Composition Pattern

**Location**: [`crates/aura-protocol/src/authorization_bridge.rs`](../crates/aura-protocol/src/authorization_bridge.rs)

**Purpose**: Combines authentication (WHO) with authorization (WHAT) without coupling the layers.

**Core Integration Function**:
```rust
pub fn authenticate_and_authorize(
    identity_proof: IdentityProof,           // FROM aura-verify
    message: &[u8],
    key_material: &KeyMaterial,
    authz_context: AuthorizationContext,     // FROM aura-wot
    operation: TreeOp,
    additional_signers: BTreeSet<DeviceId>,
    guardian_signers: BTreeSet<GuardianId>,
) -> Result<PermissionGrant, AuthorizationError> {
    // Step 1: Authentication - verify identity (WHO)
    let verified_identity = aura_verify::verify_identity_proof(
        &identity_proof, 
        message, 
        key_material
    )?;
    
    // Step 2: Authorization - evaluate capabilities (WHAT)  
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation,
        context: authz_context,
        additional_context: /* ... */
    };
    
    aura_wot::evaluate_authorization(authz_request)
}
```

### Integration Principles

1. **Linear Data Flow**: `IdentityProof` → Authentication → Authorization → `PermissionGrant`
2. **Zero Coupling**: Auth layers don't import each other, only bridge imports both
3. **Composable**: Bridge can be bypassed for scenarios requiring only auth or only authz
4. **Testable**: Each layer tested independently, bridge tested with mocks

### Send-Site Predicate and Guard Chain

At every send site in a choreography, the runtime enforces a uniform predicate and guard order:

- Predicate: `need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)`
- Guard chain: `CapGuard` → `FlowGuard` → `JournalCoupler`

Named invariants:
- Charge‑Before‑Send and No‑Observable‑Without‑Charge. If a guard fails, the step is handled locally and no packet is emitted.

See also: `docs/002_system_architecture.md` (§1.6 Guard Chain and Predicate) and `docs/001_theoretical_foundations.md` (§2.4, §5.3).

---

## Effect System Integration

### Unified Effect Interfaces

The auth/authz layers integrate with Aura's effect system through unified traits:

**Agent-Level Authentication Effects**: [`crates/aura-agent/src/handlers/auth.rs`](../crates/aura-agent/src/handlers/auth.rs)
```rust
#[async_trait] 
pub trait AuthenticationEffects: Send + Sync {
    async fn authenticate_device(&self) -> Result<AuthenticationResult>;
    async fn is_authenticated(&self) -> Result<bool>;
    async fn get_session_ticket(&self) -> Result<Option<SessionTicket>>;
}
```

**Protocol-Level Agent Effects**: [`crates/aura-protocol/src/effects/agent.rs`](../crates/aura-protocol/src/effects/agent.rs)
```rust
#[async_trait]
pub trait AgentEffects: Send + Sync {
    async fn verify_capability(&self, capability: &[u8]) -> Result<bool>;
    async fn evaluate_tree_operation(&self, op: &TreeOp) -> Result<PermissionGrant>;
    // Unified operations using authorization bridge
    async fn authorize_operation(&self, request: AuthorizedOperationRequest) -> Result<PermissionGrant>;
}
```

### Runtime Composition

**AuraEffectSystem Integration**:
```rust
// Authentication and authorization through unified effect system
let effects = AuraEffectSystem::for_production(device_id)?;

// Composed auth/authz operation
let result = effects.authorize_operation(AuthorizedOperationRequest {
    identity_proof,
    operation: TreeOp::AddLeaf { leaf, under },
    message: &signed_message,
    context: authz_context,
}).await?;
```

---

## Implementation Status

### ✅ **Working Components**:
- **aura-verify**: Pure cryptographic identity verification with all proof types
- **aura-wot**: Capability-based authorization with verified semilattice properties  
- **Authorization bridge**: Clean integration pattern with zero coupling
- **Effect system integration**: Unified agent and protocol effect traits
- **Property-based testing**: Semilattice law verification for capabilities

### ⚠️ **In Progress**:
- **aura-authenticate**: Choreographic protocol infrastructure exists, ceremony implementations pending
- **Advanced policies**: Fine-grained capability delegation and complex policy evaluation
- **Session management**: Session lifecycle and token management

### ❌ **Planned**:
- **Advanced recovery flows**: Complex guardian coordination ceremonies
- **Conditional capabilities**: Time-based, location-based capability constraints
- **Service signers**: Delegated authority for specific operations

---

## Usage Patterns

### Direct Layer Usage

**Pure Authentication**:
```rust
use aura_verify::{verify_identity_proof, IdentityProof};

let proof = IdentityProof::Device { device_id, signature };
let verified = verify_identity_proof(&proof, message, &key_material)?;
```

**Pure Authorization**:
```rust
use aura_wot::{evaluate_tree_operation_capabilities, AuthorizationContext};

let grant = evaluate_tree_operation_capabilities(&tree_op, &authz_context, &policies)?;
```

### Integrated Usage

**Complete Auth/Authz Flow**:
```rust
use aura_protocol::authorization_bridge::authenticate_and_authorize;

let permission = authenticate_and_authorize(
    identity_proof, 
    message, 
    &key_material,
    authz_context,
    tree_op,
    additional_signers,
    guardian_signers
)?;
```

**Effect System Usage**:
```rust
let effects = AuraEffectSystem::for_production(device_id)?;
let result = effects.authorize_operation(request).await?;
```

---

## Design Principles

### Separation of Concerns
- **Authentication**: Stateless identity verification, no policy knowledge
- **Authorization**: Policy evaluation, no cryptographic operations
- **Bridge**: Orchestrates composition without duplicating logic

### Formal Foundations
- **Meet-semilattice capabilities**: Mathematically sound capability attenuation
- **Property-based testing**: Algebraic laws verified automatically
- **Choreographic protocols**: Deadlock-free distributed coordination

### Integration Patterns
- **Dependency injection**: Layers consume each other through interfaces
- **Effect system**: Unified runtime access through composable handlers
- **Zero coupling**: Clean boundaries with testable integration points

---

## See Also

- [`docs/099_glossary.md`](099_glossary.md) - Complete architectural terminology
- [`docs/002_system_architecture.md`](002_system_architecture.md) - Effect system integration details
- [`docs/105_journal.md`](105_journal.md) - Journal vs Ledger layer separation
